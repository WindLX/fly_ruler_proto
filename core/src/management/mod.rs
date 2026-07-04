//! HTTP/WebSocket management API and persistence operation orchestration.

use std::collections::VecDeque;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{CONTENT_TYPE, ORIGIN};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Notify, RwLock as AsyncRwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ManagementConfig;
use crate::pb;
use crate::playback::{PlaybackController, PlaybackError};
use crate::store::{
    active_time_bounds, aircraft_count_for, event_count_for, state_count_for, Event,
    TimeSeriesStore, TimestampedEvent, TimestampedState,
};
use crate::transport::SessionHandle;
use crate::utils::now_secs;
use crate::PROTOCOL_VERSION;

mod gate;
pub use gate::IngestionGate;
mod series;
mod workspace;

use series::{SeriesError, SeriesQueryRequest};
use workspace::{WorkspaceError, WorkspaceSnapshot, WorkspaceStore};

const MAX_PAGE_LIMIT: usize = 10_000;
const DEFAULT_PAGE_LIMIT: usize = 1_000;
const MAX_WS_AIRCRAFT: usize = 64;
const MAX_OPERATIONS: usize = 128;

/// Persistence operation lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Accepted but not yet executing.
    Queued,
    /// Currently executing.
    Running,
    /// Completed successfully.
    Succeeded,
    /// Completed with an error.
    Failed,
}

/// Public persistence operation status.
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    /// Operation UUID.
    pub id: String,
    /// Operation kind (`save` or `load`).
    pub kind: String,
    /// Session name.
    pub session: String,
    /// Current lifecycle state.
    pub state: OperationState,
    /// Creation timestamp.
    pub created_at_secs: f64,
    /// Last update timestamp.
    pub updated_at_secs: f64,
    /// Error message for failed operations.
    pub error: Option<String>,
}

/// Management server startup/runtime errors.
#[derive(Debug, Error)]
pub enum ManagementError {
    /// Socket or filesystem I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Invalid server configuration.
    #[error("invalid management configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Clone)]
struct AppState {
    store: Arc<TimeSeriesStore>,
    playback: Arc<PlaybackController>,
    ingestion: Arc<IngestionGate>,
    sessions: Arc<AsyncRwLock<Option<SessionHandle>>>,
    config: ManagementConfig,
    operations: OperationManager,
    workspace: Arc<WorkspaceStore>,
    shutdown: CancellationToken,
}

#[derive(Clone)]
struct OperationManager {
    active: Arc<AtomicBool>,
    coordination: Arc<Mutex<()>>,
    records: Arc<Mutex<VecDeque<OperationRecord>>>,
    notifications: broadcast::Sender<Value>,
    idle: Arc<Notify>,
}

impl OperationManager {
    fn new() -> Self {
        let (notifications, _) = broadcast::channel(64);
        Self {
            active: Arc::new(AtomicBool::new(false)),
            coordination: Arc::new(Mutex::new(())),
            records: Arc::new(Mutex::new(VecDeque::new())),
            notifications,
            idle: Arc::new(Notify::new()),
        }
    }

    fn begin(&self, kind: &str, session: &str) -> Result<OperationRecord, ApiError> {
        let _coordination = lock_unpoisoned(&self.coordination);
        if self
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ApiError::conflict(
                "operation_busy",
                "another persistence operation is already running",
            ));
        }
        let now = now_secs();
        let record = OperationRecord {
            id: Uuid::new_v4().simple().to_string(),
            kind: kind.to_string(),
            session: session.to_string(),
            state: OperationState::Queued,
            created_at_secs: now,
            updated_at_secs: now,
            error: None,
        };
        let mut records = lock_unpoisoned(&self.records);
        records.push_back(record.clone());
        while records.len() > MAX_OPERATIONS {
            records.pop_front();
        }
        drop(records);
        let _ = self.notifications.send(json!({
            "type": "operation_status",
            "operation": record,
        }));
        Ok(record)
    }

    fn update(&self, id: &str, state: OperationState, error: Option<String>) {
        let updated = {
            let mut records = lock_unpoisoned(&self.records);
            let Some(record) = records.iter_mut().find(|record| record.id == id) else {
                return;
            };
            record.state = state;
            record.error = error;
            record.updated_at_secs = now_secs();
            record.clone()
        };
        let _ = self.notifications.send(json!({
            "type": "operation_status",
            "operation": updated,
        }));
        if matches!(state, OperationState::Succeeded | OperationState::Failed) {
            let _coordination = lock_unpoisoned(&self.coordination);
            self.active.store(false, Ordering::Release);
            self.idle.notify_waiters();
        }
    }

    fn get(&self, id: &str) -> Option<OperationRecord> {
        lock_unpoisoned(&self.records)
            .iter()
            .find(|record| record.id == id)
            .cloned()
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn notify_store_changed(&self, reason: &str) {
        let _ = self.notifications.send(json!({
            "type": "store_changed",
            "reason": reason,
        }));
    }

    fn notify_workspace_changed(&self, revision: u64) {
        let _ = self.notifications.send(json!({
            "type": "workspace_changed",
            "revision": revision,
        }));
    }

    fn run_when_idle<T>(&self, action: impl FnOnce() -> T) -> Result<T, ApiError> {
        let _coordination = lock_unpoisoned(&self.coordination);
        if self.is_active() {
            return Err(ApiError::conflict(
                "operation_busy",
                "a persistence operation is running",
            ));
        }
        Ok(action())
    }

    async fn wait_idle(&self) {
        while self.is_active() {
            let notified = self.idle.notified();
            if !self.is_active() {
                break;
            }
            notified.await;
        }
    }
}

/// Running Axum management server.
pub struct ManagementServerRuntime {
    local_addr: SocketAddr,
    shutdown: CancellationToken,
    operations: OperationManager,
    task: Option<JoinHandle<()>>,
}

impl ManagementServerRuntime {
    /// Start a management server over shared kernel state.
    pub async fn start(
        addr: &str,
        config: ManagementConfig,
        store: Arc<TimeSeriesStore>,
        playback: Arc<PlaybackController>,
        ingestion: Arc<IngestionGate>,
        sessions: Arc<AsyncRwLock<Option<SessionHandle>>>,
    ) -> Result<Self, ManagementError> {
        validate_management_config(&config)?;
        fs::create_dir_all(&config.data_root)?;
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        if !local_addr.ip().is_loopback() {
            return Err(ManagementError::InvalidConfig(format!(
                "management server must bind to a loopback address, got {local_addr}"
            )));
        }
        let operations = OperationManager::new();
        let shutdown = CancellationToken::new();
        let app_state = AppState {
            store,
            playback,
            ingestion,
            sessions,
            config: config.clone(),
            operations: operations.clone(),
            workspace: Arc::new(WorkspaceStore::new(&config.data_root)),
            shutdown: shutdown.clone(),
        };

        let app = configured_router(app_state)?;
        let serve_shutdown = shutdown.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(serve_shutdown.cancelled_owned())
                .await
            {
                warn!(target: "fly_ruler_proto_core.management", %error, "management server stopped with error");
            }
        });
        info!(target: "fly_ruler_proto_core.management", %local_addr, "management server started");
        Ok(Self {
            local_addr,
            shutdown,
            operations,
            task: Some(task),
        })
    }

    /// Return the bound local address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop the management server.
    pub async fn stop(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.operations.wait_idle().await;
    }
}

fn configured_router(state: AppState) -> Result<Router, ManagementError> {
    let origins = state
        .config
        .cors_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|_| {
                ManagementError::InvalidConfig(format!("invalid CORS origin: {origin}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT])
        .allow_headers([CONTENT_TYPE, ORIGIN]);

    let web_root = state.config.web_root.clone();
    let mut app = router(state);
    if let Some(root) = web_root.filter(|root| root.join("index.html").is_file()) {
        let static_files = ServeDir::new(&root).fallback(ServeFile::new(root.join("index.html")));
        app = app.fallback_service(static_files);
    } else {
        app = app.fallback(route_not_found);
    }
    Ok(app.layer(cors).layer(TraceLayer::new_for_http()))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/aircraft", get(aircraft_list))
        .route("/api/v1/aircraft/{id}/state", get(aircraft_state))
        .route("/api/v1/aircraft/{id}/states", get(aircraft_states))
        .route("/api/v1/aircraft/{id}/events", get(aircraft_events))
        .route("/api/v1/aircraft/{id}/series/catalog", get(series_catalog))
        .route("/api/v1/series/query", post(series_query))
        .route("/api/v1/playback", get(playback_status))
        .route("/api/v1/playback/live", post(playback_live))
        .route("/api/v1/playback/pause", post(playback_pause))
        .route("/api/v1/playback/play", post(playback_play))
        .route("/api/v1/playback/seek", post(playback_seek))
        .route("/api/v1/playback/speed", put(playback_speed))
        .route("/api/v1/memory/clear", post(memory_clear))
        .route("/api/v1/sessions", get(session_list))
        .route("/api/v1/sessions/{name}/save", post(session_save))
        .route("/api/v1/sessions/{name}/load", post(session_load))
        .route("/api/v1/operations/{id}", get(operation_status))
        .route("/api/v1/workspace", get(workspace_get).put(workspace_put))
        .route("/api/v1/ws", get(websocket))
        .route("/api", any(route_not_found))
        .route("/api/{*path}", any(route_not_found))
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

async fn route_not_found() -> ApiError {
    ApiError::not_found("route_not_found", "API route not found")
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed(
        "method_not_allowed",
        "HTTP method not allowed for this route",
    )
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "protocol_version": PROTOCOL_VERSION,
        "api_version": "v1",
    }))
}

async fn status(State(state): State<AppState>) -> Json<Value> {
    let sessions = match state.sessions.read().await.clone() {
        Some(handle) => handle.active_sessions().await,
        None => Vec::new(),
    };
    Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "api_version": "v1",
        "store": store_stats_json(&state),
        "playback": state.playback.snapshot(),
        "ingestion": {
            "enabled": state.ingestion.is_enabled(),
            "dropped_during_maintenance": state.ingestion.dropped_count(),
        },
        "persistence_operation_active": state.operations.is_active(),
        "udp_sessions": sessions.into_iter().map(|session| json!({
            "addr": session.addr.to_string(),
            "client_uuid_hex": session.client_uuid_hex,
            "last_seen_secs": session.last_seen_secs,
        })).collect::<Vec<_>>(),
    }))
}

async fn aircraft_list(State(state): State<AppState>) -> Json<Value> {
    let playback = state.playback.snapshot();
    let cursor = playback.cursor_secs;
    let aircraft = state
        .store
        .aircraft_summaries()
        .into_iter()
        .map(|summary| {
            json!({
                "id": summary.id,
                "name": summary.config.as_ref().map(|config| config.name.clone()),
                "toml_config": summary.config.as_ref().map(|config| config.toml_config.clone()),
                "time_range": summary.time_range,
                "state_count": summary.state_count,
                "event_count": summary.event_count,
                "spawned_at_cursor": cursor.is_some_and(|cursor| state.store.is_spawned_at(&summary.id, cursor)),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "aircraft": aircraft }))
}

#[derive(Deserialize)]
struct StateQuery {
    at: Option<f64>,
}

async fn aircraft_state(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    query: Result<Query<StateQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let query = query_body(query)?;
    let resolved = if let Some(timestamp) = query.at {
        if !timestamp.is_finite() {
            return Err(ApiError::bad_request(
                "invalid_timestamp",
                "at must be finite",
            ));
        }
        state
            .store
            .get_state_at_or_before(&id, timestamp)
            .map(|sample| {
                let spawned = state.store.is_spawned_at(&id, timestamp);
                (sample, spawned)
            })
    } else {
        state
            .playback
            .resolve_aircraft(&id)
            .map(|resolved| (resolved.sample, resolved.spawned))
    }
    .ok_or_else(|| ApiError::not_found("state_not_found", "aircraft state not found"))?;

    Ok(Json(json!({
        "aircraft_id": id,
        "spawned": resolved.1,
        "sample": timestamped_state_json(&resolved.0),
    })))
}

#[derive(Deserialize)]
struct RangeQuery {
    start: f64,
    end: f64,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn validated_page(query: &RangeQuery) -> Result<(usize, usize), ApiError> {
    if !query.start.is_finite() || !query.end.is_finite() || query.start > query.end {
        return Err(ApiError::bad_request(
            "invalid_range",
            "start and end must be finite and start <= end",
        ));
    }
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(ApiError::bad_request(
            "invalid_limit",
            "limit must be within 1..=10000",
        ));
    }
    Ok((query.offset.unwrap_or(0), limit))
}

async fn aircraft_states(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    query: Result<Query<RangeQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let query = query_body(query)?;
    let (offset, limit) = validated_page(&query)?;
    let page = state
        .store
        .get_states_page(&id, query.start, query.end, offset, limit)
        .ok_or_else(|| ApiError::not_found("aircraft_not_found", "aircraft not found"))?;
    Ok(Json(json!({
        "aircraft_id": id,
        "total": page.total,
        "offset": page.offset,
        "limit": page.limit,
        "items": page.items.iter().map(timestamped_state_json).collect::<Vec<_>>(),
    })))
}

async fn aircraft_events(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    query: Result<Query<RangeQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let query = query_body(query)?;
    let (offset, limit) = validated_page(&query)?;
    let page = state
        .store
        .get_events_page(&id, query.start, query.end, offset, limit)
        .ok_or_else(|| ApiError::not_found("aircraft_not_found", "aircraft not found"))?;
    Ok(Json(json!({
        "aircraft_id": id,
        "total": page.total,
        "offset": page.offset,
        "limit": page.limit,
        "items": page.items.iter().map(timestamped_event_json).collect::<Vec<_>>(),
    })))
}

async fn series_catalog(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let store = Arc::clone(&state.store);
    let aircraft_id = id.clone();
    let catalog = tokio::task::spawn_blocking(move || series::catalog(&store, &aircraft_id))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::from)?;
    Ok(Json(json!({ "aircraft_id": id, "fields": catalog })))
}

async fn series_query(
    State(state): State<AppState>,
    body: Result<Json<SeriesQueryRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let request = json_body(body)?;
    let store = Arc::clone(&state.store);
    let response = tokio::task::spawn_blocking(move || series::query(&store, request))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::from)?;
    Ok(Json(json!(response)))
}

async fn playback_status(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.playback.snapshot()))
}

async fn playback_live(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.playback.live()))
}

async fn playback_pause(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(state
        .playback
        .pause()
        .map_err(ApiError::from)?)))
}

#[derive(Deserialize)]
struct PlayRequest {
    speed: Option<f64>,
}

async fn playback_play(
    State(state): State<AppState>,
    body: Result<Json<PlayRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let speed = json_body(body)?.speed;
    Ok(Json(json!(state
        .playback
        .play(speed)
        .map_err(ApiError::from)?)))
}

#[derive(Deserialize)]
struct SeekRequest {
    timestamp: f64,
}

async fn playback_seek(
    State(state): State<AppState>,
    body: Result<Json<SeekRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let body = json_body(body)?;
    Ok(Json(json!(state
        .playback
        .seek(body.timestamp)
        .map_err(ApiError::from)?)))
}

#[derive(Deserialize)]
struct SpeedRequest {
    speed: f64,
}

async fn playback_speed(
    State(state): State<AppState>,
    body: Result<Json<SpeedRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let body = json_body(body)?;
    Ok(Json(json!(state
        .playback
        .set_speed(body.speed)
        .map_err(ApiError::from)?)))
}

#[derive(Deserialize)]
struct ConfirmRequest {
    confirm: bool,
}

async fn memory_clear(
    State(state): State<AppState>,
    body: Result<Json<ConfirmRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let body = json_body(body)?;
    if !body.confirm {
        return Err(ApiError::bad_request(
            "confirmation_required",
            "confirm must be true",
        ));
    }
    let playback = state.operations.run_when_idle(|| {
        state.ingestion.with_paused(|| {
            state.store.clear();
            state.playback.reset_empty()
        })
    })?;
    state.operations.notify_store_changed("clear");
    Ok(Json(json!({ "cleared": true, "playback": playback })))
}

async fn session_list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&state.config.data_root).map_err(ApiError::internal)? {
        let entry = entry.map_err(ApiError::internal)?;
        if !entry.file_type().map_err(ApiError::internal)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && (name.contains(".tmp-") || name.contains(".bak-")) {
            continue;
        }
        let meta_path = entry.path().join("meta.json");
        if !meta_path.is_file() {
            continue;
        }
        let metadata = fs::read(&meta_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        sessions.push(json!({ "name": name, "metadata": metadata }));
    }
    sessions.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });
    Ok(Json(json!({ "sessions": sessions })))
}

#[derive(Default, Deserialize)]
struct SaveRequest {
    #[serde(default)]
    overwrite: bool,
}

async fn session_save(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    body: Result<Json<SaveRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    validate_session_name(&name)?;
    let overwrite = json_body(body)?.overwrite;
    let target = state.config.data_root.join(&name);
    if target.exists() && !overwrite {
        return Err(ApiError::conflict(
            "session_exists",
            "session already exists; set overwrite=true to replace it",
        ));
    }
    let record = state.operations.begin("save", &name)?;
    let operation_id = record.id.clone();
    let task_state = state.clone();
    tokio::spawn(async move {
        task_state
            .operations
            .update(&operation_id, OperationState::Running, None);
        let blocking_state = task_state.clone();
        let blocking_operation_id = operation_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let snapshot = blocking_state
                .ingestion
                .with_paused(|| blocking_state.store.snapshot_clone());
            save_snapshot_atomic(
                &snapshot,
                &blocking_state.config.data_root,
                &name,
                overwrite,
                &blocking_operation_id,
            )
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
        match result {
            Ok(()) => task_state
                .operations
                .update(&operation_id, OperationState::Succeeded, None),
            Err(error) => {
                task_state
                    .operations
                    .update(&operation_id, OperationState::Failed, Some(error))
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "operation_id": record.id })),
    ))
}

async fn session_load(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    validate_session_name(&name)?;
    let target = checked_existing_session_path(&state.config.data_root, &name)?;
    if !target.is_dir() {
        return Err(ApiError::not_found(
            "session_not_found",
            "session directory not found",
        ));
    }
    let record = state.operations.begin("load", &name)?;
    let operation_id = record.id.clone();
    let task_state = state.clone();
    tokio::spawn(async move {
        task_state
            .operations
            .update(&operation_id, OperationState::Running, None);
        let load_path = target;
        let blocking_state = task_state.clone();
        let result = tokio::task::spawn_blocking(move || {
            let loaded = TimeSeriesStore::new();
            loaded.load_from_disk(&load_path)?;
            blocking_state.ingestion.with_paused(|| {
                blocking_state.store.replace_from(&loaded);
                blocking_state.playback.reset_after_load();
            });
            Ok::<(), crate::store::StoreError>(())
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
        match result {
            Ok(()) => {
                task_state
                    .operations
                    .update(&operation_id, OperationState::Succeeded, None);
                task_state.operations.notify_store_changed("load");
            }
            Err(error) => {
                task_state
                    .operations
                    .update(&operation_id, OperationState::Failed, Some(error))
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "operation_id": record.id })),
    ))
}

async fn operation_status(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let operation = state
        .operations
        .get(&id)
        .ok_or_else(|| ApiError::not_found("operation_not_found", "operation not found"))?;
    Ok(Json(json!({ "operation": operation })))
}

async fn workspace_get(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let workspace = Arc::clone(&state.workspace);
    let document = tokio::task::spawn_blocking(move || workspace.load())
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::from)?;
    Ok(Json(json!({ "workspace": document })))
}

async fn workspace_put(
    State(state): State<AppState>,
    body: Result<Json<WorkspaceSnapshot>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let snapshot = json_body(body)?;
    let workspace = Arc::clone(&state.workspace);
    let document = tokio::task::spawn_blocking(move || workspace.save(snapshot))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::from)?;
    state.operations.notify_workspace_changed(document.revision);
    Ok(Json(json!({ "workspace": document })))
}

#[derive(Deserialize)]
struct WebSocketQuery {
    aircraft: Option<String>,
}

async fn websocket(
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    State(state): State<AppState>,
    query: Result<Query<WebSocketQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let ws =
        ws.map_err(|error| ApiError::bad_request("websocket_upgrade_required", error.body_text()))?;
    let query = query_body(query)?;
    let requested = query.aircraft.map(|value| {
        value
            .split(',')
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    Ok(ws
        .max_message_size(16 * 1024)
        .on_upgrade(move |socket| websocket_loop(socket, state, requested))
        .into_response())
}

async fn websocket_loop(mut socket: WebSocket, state: AppState, requested: Option<Vec<String>>) {
    if send_ws_json(
        &mut socket,
        &json!({
            "type": "hello",
            "api_version": "v1",
            "protocol_version": PROTOCOL_VERSION,
        }),
    )
    .await
    .is_err()
    {
        return;
    }
    let period = Duration::from_secs_f64(1.0 / state.config.websocket_hz);
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut notifications = state.operations.notifications.subscribe();
    let mut sequence = 0_u64;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                sequence = sequence.wrapping_add(1);
                let snapshot = websocket_snapshot(&state, requested.as_deref(), sequence);
                if send_ws_json(&mut socket, &snapshot).await.is_err() {
                    break;
                }
            }
            notification = notifications.recv() => {
                if let Ok(notification) = notification {
                    if send_ws_json(&mut socket, &notification).await.is_err() {
                        break;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                        let error = json!({
                            "type": "error",
                            "code": "websocket_read_only",
                            "message": "playback commands must use the REST API",
                        });
                        if send_ws_json(&mut socket, &error).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) => break,
                }
            }
            _ = state.shutdown.cancelled() => break,
        }
    }
}

fn websocket_snapshot(state: &AppState, requested: Option<&[String]>, sequence: u64) -> Value {
    let mut ids = requested.map_or_else(|| state.store.get_aircraft_ids(), |ids| ids.to_vec());
    let truncated = ids.len() > MAX_WS_AIRCRAFT;
    ids.truncate(MAX_WS_AIRCRAFT);
    let playback = state.playback.snapshot();
    let aircraft = ids
        .into_iter()
        .filter_map(|id| {
            state
                .playback
                .resolve_aircraft_with(&playback, &id)
                .map(|resolved| {
                    (
                        id,
                        json!({
                            "spawned": resolved.spawned,
                            "sample": timestamped_state_json(&resolved.sample),
                        }),
                    )
                })
        })
        .collect::<Map<_, _>>();
    json!({
        "type": "snapshot",
        "sequence": sequence,
        "server_time_secs": now_secs(),
        "playback": playback,
        "store": store_stats_json(state),
        "aircraft": aircraft,
        "truncated": truncated,
    })
}

async fn send_ws_json(socket: &mut WebSocket, value: &Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}

fn store_stats_json(state: &AppState) -> Value {
    json!({
        "aircraft_count": aircraft_count_for(&state.store),
        "state_count": state_count_for(&state.store),
        "event_count": event_count_for(&state.store),
        "time_bounds": active_time_bounds(&state.store),
    })
}

fn timestamped_state_json(sample: &TimestampedState) -> Value {
    json!({
        "timestamp_secs": sample.timestamp_secs,
        "state": aircraft_state_json(&sample.state),
    })
}

fn timestamped_event_json(event: &TimestampedEvent) -> Value {
    match &event.event {
        Event::Spawn(spawn) => json!({
            "timestamp_secs": event.timestamp_secs,
            "event_type": "spawn",
            "name": spawn.name,
            "toml_config": spawn.toml_config,
            "initial_state": spawn.initial_state.as_ref().map(aircraft_state_json),
        }),
        Event::Despawn(despawn) => json!({
            "timestamp_secs": event.timestamp_secs,
            "event_type": "despawn",
            "reason": despawn.reason,
        }),
        Event::Custom(name) => json!({
            "timestamp_secs": event.timestamp_secs,
            "event_type": "custom",
            "name": name,
        }),
    }
}

fn aircraft_state_json(state: &pb::AircraftState) -> Value {
    let custom_fields = state
        .custom_fields
        .iter()
        .filter_map(|field| {
            let kind = field.value.as_ref()?.kind.as_ref()?;
            let value = match kind {
                pb::field_value::Kind::F64Value(value) => {
                    json!({"kind": "f64", "value": value})
                }
                pb::field_value::Kind::I64Value(value) => {
                    json!({"kind": "i64", "value": value})
                }
                pb::field_value::Kind::BoolValue(value) => {
                    json!({"kind": "bool", "value": value})
                }
                pb::field_value::Kind::StringValue(value) => {
                    json!({"kind": "string", "value": value})
                }
                pb::field_value::Kind::BytesValue(value) => {
                    json!({"kind": "bytes", "value": BASE64.encode(value)})
                }
            };
            Some((field.field_id.clone(), value))
        })
        .collect::<Map<_, _>>();
    json!({
        "position": state.position.as_ref().map(vector_json),
        "velocity": state.velocity.as_ref().map(vector_json),
        "attitude": state.attitude.as_ref().map(|value| json!({
            "w": value.w, "x": value.x, "y": value.y, "z": value.z,
        })),
        "angular_velocity": state.angular_velocity.as_ref().map(vector_json),
        "derived": state.derived.as_ref().map(|value| json!({
            "lat": value.lat,
            "lon": value.lon,
            "altitude": value.altitude,
            "alpha": value.alpha,
            "beta": value.beta,
            "tas": value.tas,
            "eas": value.eas,
            "gamma": value.gamma,
            "chi": value.chi,
            "ias": value.ias,
            "cas": value.cas,
            "mach": value.mach,
        })),
        "control_surfaces": state.control_surfaces.as_ref().map(|value| json!({
            "aileron_left_rad": value.aileron_left_rad,
            "aileron_right_rad": value.aileron_right_rad,
            "elevator_rad": value.elevator_rad,
            "rudder_rad": value.rudder_rad,
            "flaps_left_ratio": value.flaps_left_ratio,
            "flaps_right_ratio": value.flaps_right_ratio,
            "spoilers_ratio": value.spoilers_ratio,
        })),
        "engines": state.engines.iter().map(|value| json!({
            "index": value.index,
            "throttle_lever_ratio": value.throttle_lever_ratio,
        })).collect::<Vec<_>>(),
        "custom_fields": custom_fields,
    })
}

fn vector_json(value: &pb::Vector3) -> Value {
    json!({"x": value.x, "y": value.y, "z": value.z})
}

fn validate_management_config(config: &ManagementConfig) -> Result<(), ManagementError> {
    if !config.websocket_hz.is_finite() || config.websocket_hz <= 0.0 {
        return Err(ManagementError::InvalidConfig(
            "websocket_hz must be finite and greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(body)| body)
        .map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))
}

fn query_body<T>(query: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    query
        .map(|Query(query)| query)
        .map_err(|error| ApiError::bad_request("invalid_query", error.body_text()))
}

fn validate_session_name(name: &str) -> Result<(), ApiError> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_session_name",
            "session name must match [A-Za-z0-9._-]{1,128} and cannot be . or ..",
        ))
    }
}

fn checked_existing_session_path(
    data_root: &Path,
    name: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let target = data_root.join(name);
    let metadata = fs::symlink_metadata(&target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found("session_not_found", "session directory not found")
        } else {
            ApiError::internal(error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ApiError::bad_request(
            "invalid_session_path",
            "session symlinks are not allowed",
        ));
    }
    let canonical_root = fs::canonicalize(data_root).map_err(ApiError::internal)?;
    let canonical_target = fs::canonicalize(&target).map_err(ApiError::internal)?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(ApiError::bad_request(
            "invalid_session_path",
            "session path escapes the configured data root",
        ));
    }
    Ok(canonical_target)
}

fn save_snapshot_atomic(
    snapshot: &TimeSeriesStore,
    data_root: &Path,
    name: &str,
    overwrite: bool,
    operation_id: &str,
) -> Result<(), crate::store::StoreError> {
    let target = data_root.join(name);
    let temporary = data_root.join(format!(".{name}.tmp-{operation_id}"));
    let backup = data_root.join(format!(".{name}.bak-{operation_id}"));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    if let Err(error) = snapshot.save_to_disk(&temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }

    if !target.exists() {
        fs::rename(&temporary, &target)?;
        return Ok(());
    }
    if !overwrite {
        fs::remove_dir_all(&temporary)?;
        return Err(crate::store::StoreError::InvalidData(
            "session already exists".to_string(),
        ));
    }
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::rename(&target, &backup)?;
    if let Err(error) = fs::rename(&temporary, &target) {
        let _ = fs::rename(&backup, &target);
        let _ = fs::remove_dir_all(&temporary);
        return Err(error.into());
    }
    fs::remove_dir_all(&backup)?;
    Ok(())
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn method_not_allowed(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: error.to_string(),
            details: None,
        }
    }
}

impl From<PlaybackError> for ApiError {
    fn from(error: PlaybackError) -> Self {
        Self {
            status: match error {
                PlaybackError::EmptyStore => StatusCode::CONFLICT,
                PlaybackError::InvalidTimestamp | PlaybackError::InvalidSpeed { .. } => {
                    StatusCode::BAD_REQUEST
                }
            },
            code: "playback_error",
            message: error.to_string(),
            details: None,
        }
    }
}

impl From<SeriesError> for ApiError {
    fn from(error: SeriesError) -> Self {
        let status = match error {
            SeriesError::AircraftNotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            code: "series_error",
            message: error.to_string(),
            details: None,
        }
    }
}

impl From<WorkspaceError> for ApiError {
    fn from(error: WorkspaceError) -> Self {
        let status = match error {
            WorkspaceError::Io(_) | WorkspaceError::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
            WorkspaceError::TooLarge
            | WorkspaceError::TooComplex
            | WorkspaceError::InvalidValue => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            code: "workspace_error",
            message: error.to_string(),
            details: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "code": self.code,
                "message": self.message,
                "details": self.details,
            })),
        )
            .into_response()
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use futures_util::{SinkExt, StreamExt};
    use tower::ServiceExt;

    #[test]
    fn validates_session_names() {
        assert!(validate_session_name("flight-01.test").is_ok());
        assert!(validate_session_name("../escape").is_err());
        assert!(validate_session_name(".").is_err());
        assert!(validate_session_name("").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_session_symlinks_outside_data_root() {
        use std::os::unix::fs::symlink;

        let root = test_root("symlink-root");
        let outside = test_root("symlink-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert!(checked_existing_session_path(&root, "escape").is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn ingestion_gate_counts_dropped_messages() {
        let gate = IngestionGate::new();
        gate.with_paused(|| assert!(gate.with_ingestion(|| ()).is_none()));
        assert!(gate.with_ingestion(|| ()).is_some());
        assert_eq!(gate.dropped_count(), 1);
    }

    #[test]
    fn persistence_operations_are_serialized() {
        let operations = OperationManager::new();
        let first = operations.begin("save", "first").unwrap();
        assert!(operations.begin("load", "second").is_err());
        operations.update(&first.id, OperationState::Succeeded, None);
        assert!(operations.begin("load", "second").is_ok());
    }

    fn test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fly-ruler-management-{label}-{}",
            Uuid::new_v4().simple()
        ))
    }

    fn test_state(root: std::path::PathBuf) -> AppState {
        fs::create_dir_all(&root).unwrap();
        let store = Arc::new(TimeSeriesStore::new());
        store.append_event(
            "aircraft-1".to_string(),
            1.0,
            Event::Spawn(Box::new(pb::AircraftSpawnInfo {
                name: "test".to_string(),
                toml_config: String::new(),
                initial_state: None,
            })),
        );
        store.append_state(
            "aircraft-1".to_string(),
            2.0,
            pb::AircraftState {
                position: Some(pb::Vector3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }),
                ..Default::default()
            },
        );
        let playback = Arc::new(PlaybackController::new(
            Arc::clone(&store),
            crate::ReplayConfig::default(),
        ));
        let workspace = Arc::new(WorkspaceStore::new(&root));
        AppState {
            store,
            playback,
            ingestion: Arc::new(IngestionGate::new()),
            sessions: Arc::new(AsyncRwLock::new(None)),
            config: ManagementConfig {
                data_root: root,
                websocket_hz: 120.0,
                ..ManagementConfig::default()
            },
            operations: OperationManager::new(),
            workspace,
            shutdown: CancellationToken::new(),
        }
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn http_routes_validate_queries_and_mutate_playback() {
        let root = test_root("http");
        let state = test_state(root.clone());
        let app = router(state.clone());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["store"]["state_count"], 1);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/aircraft/aircraft-1/states?start=0&end=3&limit=10001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/playback/seek")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"timestamp":1.5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await["mode"],
            Value::String("replay_paused".to_string())
        );

        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/playback/seek")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(response).await["code"], "invalid_json");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_json(response).await["code"], "route_not_found");

        let response = app
            .oneshot(
                Request::post("/api/v1/memory/clear")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"confirm":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.store.get_aircraft_ids().is_empty());
        assert!(state.playback.snapshot().cursor_secs.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cors_allows_default_localhost_origin() {
        use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD};

        let root = test_root("cors");
        let state = test_state(root.clone());
        let app = configured_router(state).unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/v1/status")
                    .header(ORIGIN, "http://localhost:5173")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://localhost:5173"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn serves_spa_assets_but_keeps_api_errors_json() {
        let root = test_root("static");
        let web_root = root.join("web");
        fs::create_dir_all(web_root.join("assets")).unwrap();
        fs::write(web_root.join("index.html"), b"<main>FlyRuler</main>").unwrap();
        fs::write(web_root.join("assets/app.js"), b"console.log('ok')").unwrap();
        let mut state = test_state(root.clone());
        state.config.web_root = Some(web_root);
        let app = configured_router(state).unwrap();

        let asset = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);

        let fallback = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/replay/aircraft-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(fallback.status(), StatusCode::OK);
        let fallback_body = to_bytes(fallback.into_body(), 1024).await.unwrap();
        assert_eq!(&fallback_body[..], b"<main>FlyRuler</main>");

        let api_error = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/not-real")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api_error.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_json(api_error).await["code"], "route_not_found");
        let _ = fs::remove_dir_all(root);
    }

    async fn wait_for_operation(state: &AppState, id: &str) -> OperationRecord {
        for _ in 0..200 {
            if let Some(record) = state.operations.get(id) {
                if matches!(
                    record.state,
                    OperationState::Succeeded | OperationState::Failed
                ) {
                    return record;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("operation {id} did not complete");
    }

    #[tokio::test]
    async fn async_save_clear_load_roundtrip_is_transactional() {
        let root = test_root("persistence");
        let state = test_state(root.clone());
        let app = router(state.clone());

        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/sessions/flight/save")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"overwrite":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let save_id = response_json(response).await["operation_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            wait_for_operation(&state, &save_id).await.state,
            OperationState::Succeeded
        );

        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/sessions/flight/save")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"overwrite":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        state.ingestion.with_paused(|| state.store.clear());
        state.playback.reset_empty();
        let response = app
            .oneshot(
                Request::post("/api/v1/sessions/flight/load")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let load_id = response_json(response).await["operation_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            wait_for_operation(&state, &load_id).await.state,
            OperationState::Succeeded
        );
        assert_eq!(state.store.get_aircraft_ids(), ["aircraft-1"]);
        assert_eq!(state.playback.snapshot().cursor_secs, Some(1.0));

        fs::create_dir(root.join("corrupt")).unwrap();
        fs::write(root.join("corrupt/states.parquet"), b"not parquet").unwrap();
        let response = router(state.clone())
            .oneshot(
                Request::post("/api/v1/sessions/corrupt/load")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let corrupt_load_id = response_json(response).await["operation_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            wait_for_operation(&state, &corrupt_load_id).await.state,
            OperationState::Failed
        );
        assert_eq!(state.store.get_aircraft_ids(), ["aircraft-1"]);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn websocket_is_read_only_and_emits_snapshots() {
        let root = test_root("websocket");
        let state = test_state(root.clone());
        let mut runtime = ManagementServerRuntime::start(
            "127.0.0.1:0",
            state.config.clone(),
            Arc::clone(&state.store),
            Arc::clone(&state.playback),
            Arc::clone(&state.ingestion),
            Arc::clone(&state.sessions),
        )
        .await
        .unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://{}/api/v1/ws?aircraft=aircraft-1",
            runtime.local_addr()
        ))
        .await
        .unwrap();

        let hello = socket.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&hello).unwrap()["type"],
            "hello"
        );
        let snapshot = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let snapshot = serde_json::from_str::<Value>(&snapshot).unwrap();
        assert_eq!(snapshot["type"], "snapshot");
        assert!(snapshot["aircraft"]["aircraft-1"].is_object());

        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"command":"seek"}"#.into(),
            ))
            .await
            .unwrap();
        let mut saw_read_only_error = false;
        for _ in 0..10 {
            let message = socket.next().await.unwrap().unwrap();
            let Ok(text) = message.into_text() else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["code"] == "websocket_read_only" {
                saw_read_only_error = true;
                break;
            }
        }
        assert!(saw_read_only_error);
        tokio::time::timeout(Duration::from_secs(1), runtime.stop())
            .await
            .expect("management shutdown should close active websockets");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match socket.next().await {
                    None
                    | Some(Err(_))
                    | Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
        .expect("websocket should observe management shutdown");
        let _ = fs::remove_dir_all(root);
    }
}
