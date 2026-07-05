//! HTTP/WebSocket route handlers for the management API.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::management::series::{self, SeriesQueryRequest};
use crate::management::server::{
    json_body, query_body, validate_session_name, ApiError, AppState, OperationState,
    DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, MAX_WS_AIRCRAFT,
};
use crate::management::workspace::WorkspaceSnapshot;
use crate::pb;
use crate::store::{
    active_time_bounds, aircraft_count_for, event_count_for, state_count_for, Event,
    GlobalTimestampedEvent, TimeSeriesStore, TimestampedEvent, TimestampedState,
};
use crate::utils::now_secs;
use crate::PROTOCOL_VERSION;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/aircraft", get(aircraft_list))
        .route("/api/v1/aircraft/{id}/state", get(aircraft_state))
        .route("/api/v1/aircraft/{id}/states", get(aircraft_states))
        .route("/api/v1/aircraft/{id}/events", get(aircraft_events))
        .route("/api/v1/timeline/events", get(timeline_events))
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

pub(crate) async fn route_not_found() -> ApiError {
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

async fn timeline_events(
    State(state): State<AppState>,
    query: Result<Query<RangeQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let query = query_body(query)?;
    let (offset, limit) = validated_page(&query)?;
    let page = state
        .store
        .get_global_events_page(query.start, query.end, offset, limit);
    Ok(Json(json!({
        "total": page.total,
        "offset": page.offset,
        "limit": page.limit,
        "items": page.items.iter().map(global_event_json).collect::<Vec<_>>(),
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
    let data_root = &state.config.data_root;
    if !data_root.exists() {
        fs::create_dir_all(data_root).map_err(ApiError::internal)?;
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(data_root).map_err(ApiError::internal)? {
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
            crate::management::server::save_snapshot_atomic(
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
    let target =
        crate::management::server::checked_existing_session_path(&state.config.data_root, &name)?;
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

fn global_event_json(event: &GlobalTimestampedEvent) -> Value {
    let mut value = timestamped_event_json(&TimestampedEvent {
        timestamp_secs: event.timestamp_secs,
        event: event.event.clone(),
    });
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "aircraft_id".to_string(),
            Value::String(event.aircraft_id.clone()),
        );
    }
    value
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::header::CONTENT_TYPE;
    use axum::http::{Request, StatusCode};
    use futures_util::{SinkExt, StreamExt};
    use tower::ServiceExt;

    use crate::management::server::{ManagementServerRuntime, OperationRecord};

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn http_routes_validate_queries_and_mutate_playback() {
        let root = crate::management::server::test_root("http");
        let state = crate::management::server::test_state(root.clone());
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
                    .uri("/api/v1/timeline/events?start=0&end=3&offset=0&limit=100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let timeline = response_json(response).await;
        assert_eq!(timeline["total"], 1);
        assert_eq!(timeline["items"][0]["aircraft_id"], "aircraft-1");
        assert_eq!(timeline["items"][0]["event_type"], "spawn");

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
        use axum::http::header::ORIGIN;
        use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD};
        use axum::http::Method;

        let root = crate::management::server::test_root("cors");
        let state = crate::management::server::test_state(root.clone());
        let app = crate::management::server::configured_router(state).unwrap();
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
        let root = crate::management::server::test_root("static");
        let web_root = root.join("web");
        fs::create_dir_all(web_root.join("assets")).unwrap();
        fs::write(
            web_root.join("index.html"),
            b"<main>FlyRuler</main><script type=\"application/json\">__FLY_RULER_RUNTIME_CONFIG__</script>",
        )
        .unwrap();
        fs::write(web_root.join("assets/app.js"), b"console.log('ok')").unwrap();
        let mut state = crate::management::server::test_state(root.clone());
        state.config.web_root = Some(web_root);
        state.config.public_api_base_url = Some("https://example.test/<unsafe>/api/v1".to_string());
        let app = crate::management::server::configured_router(state).unwrap();

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
        let fallback_body = String::from_utf8(fallback_body.to_vec()).unwrap();
        assert!(fallback_body.contains("<main>FlyRuler</main>"));
        assert!(fallback_body.contains("https://example.test/\\u003cunsafe\\u003e/api/v1"));
        assert!(!fallback_body.contains(crate::management::server::WEB_CONFIG_SENTINEL));

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
        let root = crate::management::server::test_root("persistence");
        let state = crate::management::server::test_state(root.clone());
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
        let root = crate::management::server::test_root("websocket");
        let state = crate::management::server::test_state(root.clone());
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
