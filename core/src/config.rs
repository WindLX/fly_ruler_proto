//! Core runtime configuration.
//!
//! This module centralizes configuration knobs used by kernel orchestration,
//! transport/session behavior and store ingestion behavior.

use std::path::PathBuf;

/// Transport/session-related runtime options.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Interval between client heartbeats, in seconds.
    pub heartbeat_interval_secs: u64,
    /// Server-side timeout after which a session is considered expired, in seconds.
    pub heartbeat_timeout_secs: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_secs: 5,
            heartbeat_timeout_secs: 15,
        }
    }
}

/// Store-related runtime options.
///
/// Currently intentionally empty; future knobs (e.g. retention, queue sizing)
/// will be added here without breaking the API.
#[derive(Debug, Clone, Default)]
pub struct StoreConfig;

/// HTTP/WebSocket management server options.
#[derive(Debug, Clone)]
pub struct ManagementConfig {
    /// Directory containing named persisted sessions.
    pub data_root: PathBuf,
    /// Optional Vite distribution directory served as a single-page app.
    pub web_root: Option<PathBuf>,
    /// Public REST API base injected into the Web console.
    ///
    /// `None` uses the same-origin `/api/v1` path.
    pub public_api_base_url: Option<String>,
    /// Public WebSocket URL injected into the Web console.
    ///
    /// `None` uses the same-origin `/api/v1/ws` path.
    pub public_websocket_url: Option<String>,
    /// Aggregate WebSocket snapshot frequency.
    pub websocket_hz: f64,
    /// Browser origins allowed to access the localhost API.
    pub cors_origins: Vec<String>,
}

impl Default for ManagementConfig {
    fn default() -> Self {
        Self {
            data_root: PathBuf::from("sessions"),
            web_root: Some(PathBuf::from("web/dist")),
            public_api_base_url: None,
            public_websocket_url: None,
            websocket_hz: 30.0,
            cors_origins: vec![
                "http://localhost:3000".to_string(),
                "http://127.0.0.1:3000".to_string(),
                "http://localhost:5173".to_string(),
                "http://127.0.0.1:5173".to_string(),
                "http://localhost:8000".to_string(),
                "http://127.0.0.1:8000".to_string(),
                "http://localhost:18003".to_string(),
                "http://127.0.0.1:18003".to_string(),
            ],
        }
    }
}

/// Global playback controller options.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Initial playback speed.
    pub default_speed: f64,
    /// Minimum accepted forward playback speed.
    pub min_speed: f64,
    /// Maximum accepted forward playback speed.
    pub max_speed: f64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            default_speed: 1.0,
            min_speed: 0.1,
            max_speed: 16.0,
        }
    }
}

/// Logging-related runtime options.
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Global log level, e.g. "trace"|"debug"|"info"|"warn"|"error".
    pub level: String,
    /// Optional log output file path. When `None`, logs go to stderr.
    pub file_path: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "warn".to_string(),
            file_path: None,
        }
    }
}

/// Top-level runtime configuration for kernel orchestration.
#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    /// Transport and session configuration.
    pub transport: TransportConfig,
    /// Store ingestion configuration.
    pub store: StoreConfig,
    /// HTTP/WebSocket management server configuration.
    pub management: ManagementConfig,
    /// Playback state machine configuration.
    pub replay: ReplayConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
}
