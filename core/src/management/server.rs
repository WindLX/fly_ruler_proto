//! HTTP/WebSocket management server runtime and shared application state.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::Query;
use axum::http::header::{CONTENT_TYPE, ORIGIN};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Notify, RwLock as AsyncRwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower::service_fn;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::ManagementConfig;
use crate::management::gate::IngestionGate;
use crate::management::routes;
use crate::management::series::SeriesError;
use crate::management::workspace::{WorkspaceError, WorkspaceStore};
use crate::playback::{PlaybackController, PlaybackError};
use crate::store::TimeSeriesStore;
use crate::transport::SessionHandle;
use crate::utils::now_secs;

pub(crate) const MAX_PAGE_LIMIT: usize = 10_000;
pub(crate) const DEFAULT_PAGE_LIMIT: usize = 1_000;
pub(crate) const MAX_WS_AIRCRAFT: usize = 64;
pub(crate) const MAX_OPERATIONS: usize = 128;
pub(crate) const WEB_CONFIG_SENTINEL: &str = "__FLY_RULER_RUNTIME_CONFIG__";

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
pub(crate) struct AppState {
    pub(crate) store: Arc<TimeSeriesStore>,
    pub(crate) playback: Arc<PlaybackController>,
    pub(crate) ingestion: Arc<IngestionGate>,
    pub(crate) sessions: Arc<AsyncRwLock<Option<SessionHandle>>>,
    pub(crate) config: ManagementConfig,
    pub(crate) operations: OperationManager,
    pub(crate) workspace: Arc<WorkspaceStore>,
    pub(crate) shutdown: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct OperationManager {
    active: Arc<AtomicBool>,
    coordination: Arc<Mutex<()>>,
    records: Arc<Mutex<VecDeque<OperationRecord>>>,
    pub(crate) notifications: broadcast::Sender<Value>,
    idle: Arc<Notify>,
}

impl OperationManager {
    pub(crate) fn new() -> Self {
        let (notifications, _) = broadcast::channel(64);
        Self {
            active: Arc::new(AtomicBool::new(false)),
            coordination: Arc::new(Mutex::new(())),
            records: Arc::new(Mutex::new(VecDeque::new())),
            notifications,
            idle: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn begin(&self, kind: &str, session: &str) -> Result<OperationRecord, ApiError> {
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

    pub(crate) fn update(&self, id: &str, state: OperationState, error: Option<String>) {
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

    pub(crate) fn get(&self, id: &str) -> Option<OperationRecord> {
        lock_unpoisoned(&self.records)
            .iter()
            .find(|record| record.id == id)
            .cloned()
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn notify_store_changed(&self, reason: &str) {
        let _ = self.notifications.send(json!({
            "type": "store_changed",
            "reason": reason,
        }));
    }

    pub(crate) fn notify_workspace_changed(&self, revision: u64) {
        let _ = self.notifications.send(json!({
            "type": "workspace_changed",
            "revision": revision,
        }));
    }

    pub(crate) fn run_when_idle<T>(&self, action: impl FnOnce() -> T) -> Result<T, ApiError> {
        let _coordination = lock_unpoisoned(&self.coordination);
        if self.is_active() {
            return Err(ApiError::conflict(
                "operation_busy",
                "a persistence operation is running",
            ));
        }
        Ok(action())
    }

    pub(crate) async fn wait_idle(&self) {
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
        mut config: ManagementConfig,
        store: Arc<TimeSeriesStore>,
        playback: Arc<PlaybackController>,
        ingestion: Arc<IngestionGate>,
        sessions: Arc<AsyncRwLock<Option<SessionHandle>>>,
    ) -> Result<Self, ManagementError> {
        validate_management_config(&config)?;
        config.data_root = resolve_data_root(&config.data_root)?;
        fs::create_dir_all(&config.data_root)?;
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
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

pub(crate) fn configured_router(state: AppState) -> Result<Router, ManagementError> {
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

    let rendered_web = load_rendered_web_index(&state.config)?;
    let mut app = routes::router(state);
    if let Some((root, index)) = rendered_web {
        let root_index = Arc::clone(&index);
        let named_index = Arc::clone(&index);
        app = app
            .route(
                "/",
                get(move || {
                    let index = Arc::clone(&root_index);
                    async move { html_response(index) }
                }),
            )
            .route(
                "/index.html",
                get(move || {
                    let index = Arc::clone(&named_index);
                    async move { html_response(index) }
                }),
            );
        let fallback_index = Arc::clone(&index);
        let fallback = service_fn(move |_request: Request<Body>| {
            let index = Arc::clone(&fallback_index);
            async move { Ok::<_, Infallible>(html_response(index)) }
        });
        let static_files = ServeDir::new(&root)
            .append_index_html_on_directories(false)
            .fallback(fallback);
        app = app.fallback_service(static_files);
    } else {
        app = app.fallback(routes::route_not_found);
    }
    Ok(app.layer(cors).layer(TraceLayer::new_for_http()))
}

fn resolve_data_root(data_root: &Path) -> Result<PathBuf, ManagementError> {
    let absolute = if data_root.is_absolute() {
        data_root.to_path_buf()
    } else {
        std::env::current_dir()?.join(data_root)
    };
    // `fs::canonicalize` requires the path to exist. The directory is created
    // right after this call, so failure here only means missing intermediate
    // directories or platform quirks (e.g. Proton/Windows path mapping). In
    // that case the absolute path is still usable.
    Ok(fs::canonicalize(&absolute).unwrap_or(absolute))
}

pub(crate) fn validate_management_config(config: &ManagementConfig) -> Result<(), ManagementError> {
    if !config.websocket_hz.is_finite() || config.websocket_hz <= 0.0 {
        return Err(ManagementError::InvalidConfig(
            "websocket_hz must be finite and greater than zero".to_string(),
        ));
    }
    validate_public_url(
        config.public_api_base_url.as_deref(),
        &["http://", "https://", "/"],
        "public_api_base_url",
    )?;
    validate_public_url(
        config.public_websocket_url.as_deref(),
        &["ws://", "wss://", "/"],
        "public_websocket_url",
    )?;
    Ok(())
}

fn validate_public_url(
    value: Option<&str>,
    prefixes: &[&str],
    field: &str,
) -> Result<(), ManagementError> {
    if let Some(value) = value {
        let valid_prefix = prefixes.iter().any(|prefix| {
            value.starts_with(prefix) && (*prefix != "/" || !value.starts_with("//"))
        });
        if value.is_empty() || !valid_prefix {
            return Err(ManagementError::InvalidConfig(format!(
                "{field} must be an absolute URL or a root-relative path"
            )));
        }
    }
    Ok(())
}

pub(crate) fn load_rendered_web_index(
    config: &ManagementConfig,
) -> Result<Option<(std::path::PathBuf, Arc<String>)>, ManagementError> {
    let Some(root) = config
        .web_root
        .as_ref()
        .filter(|root| root.join("index.html").is_file())
    else {
        return Ok(None);
    };
    let source = fs::read_to_string(root.join("index.html"))?;
    if !source.contains(WEB_CONFIG_SENTINEL) {
        return Err(ManagementError::InvalidConfig(format!(
            "{} does not contain the runtime configuration placeholder",
            root.join("index.html").display()
        )));
    }
    let runtime = json!({
        "api_base_url": config.public_api_base_url.as_deref().unwrap_or("/api/v1"),
        "websocket_url": config.public_websocket_url.as_deref().unwrap_or("/api/v1/ws"),
    });
    let escaped = serde_json::to_string(&runtime)
        .expect("runtime Web configuration is serializable")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    Ok(Some((
        root.clone(),
        Arc::new(source.replace(WEB_CONFIG_SENTINEL, &escaped)),
    )))
}

pub(crate) fn html_response(index: Arc<String>) -> Response {
    (
        [(CONTENT_TYPE, "text/html; charset=utf-8")],
        index.as_str().to_owned(),
    )
        .into_response()
}

pub(crate) fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(body)| body)
        .map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))
}

pub(crate) fn query_body<T>(query: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    query
        .map(|Query(query)| query)
        .map_err(|error| ApiError::bad_request("invalid_query", error.body_text()))
}

pub(crate) fn validate_session_name(name: &str) -> Result<(), ApiError> {
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

pub(crate) fn checked_existing_session_path(
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

pub(crate) fn save_snapshot_atomic(
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
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    pub(crate) fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn method_not_allowed(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
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
                PlaybackError::InvalidTimestamp
                | PlaybackError::InvalidSpeed { .. }
                | PlaybackError::InvalidStepCount => StatusCode::BAD_REQUEST,
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
        if self.status.is_server_error() {
            error!(
                target: "fly_ruler_proto_core.management",
                status = %self.status,
                code = self.code,
                message = %self.message,
                "management request failed"
            );
        }
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

pub(crate) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) fn test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "fly-ruler-management-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

#[cfg(test)]
pub(crate) fn test_state(root: std::path::PathBuf) -> AppState {
    use crate::management::workspace::WorkspaceStore;
    use crate::pb;
    use crate::playback::PlaybackController;
    use crate::store::{Event, TimeSeriesStore};

    fs::create_dir_all(&root).unwrap();
    let store = Arc::new(TimeSeriesStore::new());
    store.append_event(
        "aircraft-1".to_string(),
        1.0,
        Event::Spawn(Box::new(pb::AircraftSpawnInfo {
            name: "test".to_string(),
            toml_config: String::new(),
            initial_state: None,
            telemetry_schemas: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn validates_public_web_urls() {
        let invalid = ManagementConfig {
            public_api_base_url: Some("ftp://example.test/api".to_string()),
            ..ManagementConfig::default()
        };
        assert!(validate_management_config(&invalid).is_err());
        let valid = ManagementConfig {
            public_api_base_url: Some("/api/v1".to_string()),
            public_websocket_url: Some("wss://example.test/ws".to_string()),
            ..ManagementConfig::default()
        };
        validate_management_config(&valid).unwrap();
    }
}
