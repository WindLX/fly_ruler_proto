//! Core runtime configuration.
//!
//! This module centralizes configuration knobs used by kernel orchestration,
//! transport/session behavior and store ingestion behavior.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Current version of the shared runtime TOML schema.
pub const RUNTIME_CONFIG_SCHEMA_VERSION: u32 = 2;

/// Shared host-runtime sections persisted by bridge applications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeFileConfig {
    /// TOML schema version.
    pub schema_version: u32,
    /// UDP transport settings.
    pub transport: TransportFileConfig,
    /// Cursor room publication/subscription settings.
    pub cursor_stream: CursorStreamFileConfig,
    /// HTTP/WebSocket management settings.
    pub management: ManagementFileConfig,
    /// Playback limits.
    pub playback: PlaybackFileConfig,
    /// Logging settings.
    pub logging: LoggingFileConfig,
}

impl Default for RuntimeFileConfig {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_CONFIG_SCHEMA_VERSION,
            transport: TransportFileConfig::default(),
            cursor_stream: CursorStreamFileConfig::default(),
            management: ManagementFileConfig::default(),
            playback: PlaybackFileConfig::default(),
            logging: LoggingFileConfig::default(),
        }
    }
}

/// Persisted UDP transport settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransportFileConfig {
    /// Runtime role: `server` or receive-only `client`.
    pub role: String,
    /// UDP bind address.
    pub udp_listen: String,
    /// Authoritative room address used in client mode.
    pub server_address: String,
    /// Client heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// Server-side session timeout in seconds.
    pub heartbeat_timeout_secs: u64,
}

impl Default for TransportFileConfig {
    fn default() -> Self {
        Self {
            role: "server".to_string(),
            udp_listen: "0.0.0.0:18002".to_string(),
            server_address: "127.0.0.1:18002".to_string(),
            heartbeat_interval_secs: 5,
            heartbeat_timeout_secs: 15,
        }
    }
}

/// Persisted cursor room streaming settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorStreamFileConfig {
    /// Server snapshot publication rate.
    pub publish_hz: f64,
    /// Maximum room observers.
    pub max_subscribers: usize,
    /// Initial client reconnect delay.
    pub reconnect_initial_secs: f64,
    /// Maximum client reconnect delay.
    pub reconnect_max_secs: f64,
}

impl Default for CursorStreamFileConfig {
    fn default() -> Self {
        Self {
            publish_hz: 30.0,
            max_subscribers: 16,
            reconnect_initial_secs: 0.5,
            reconnect_max_secs: 5.0,
        }
    }
}

/// Persisted management-server settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManagementFileConfig {
    /// Whether the management server is enabled.
    pub enabled: bool,
    /// HTTP bind address.
    pub listen: String,
    /// Session persistence root as a host-resolved path string.
    pub data_root: String,
    /// Web distribution root as a host-resolved path string.
    pub web_root: String,
    /// WebSocket snapshot publication rate.
    pub websocket_hz: f64,
}

impl Default for ManagementFileConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: "127.0.0.1:18003".to_string(),
            data_root: "sessions".to_string(),
            web_root: "web/dist".to_string(),
            websocket_hz: 30.0,
        }
    }
}

/// Persisted playback speed limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlaybackFileConfig {
    /// Minimum forward playback speed.
    pub min_speed: f64,
    /// Maximum forward playback speed.
    pub max_speed: f64,
}

impl Default for PlaybackFileConfig {
    fn default() -> Self {
        Self {
            min_speed: 0.1,
            max_speed: 16.0,
        }
    }
}

/// Persisted logging settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingFileConfig {
    /// Tracing level.
    pub level: String,
    /// Optional log file path; an empty string selects stderr.
    pub file_path: String,
}

impl Default for LoggingFileConfig {
    fn default() -> Self {
        Self {
            level: "warn".to_string(),
            file_path: String::new(),
        }
    }
}

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
    /// Authoritative cursor stream configuration.
    pub cursor_stream: crate::cursor::CursorStreamConfig,
    /// Store ingestion configuration.
    pub store: StoreConfig,
    /// HTTP/WebSocket management server configuration.
    pub management: ManagementConfig,
    /// Playback state machine configuration.
    pub replay: ReplayConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
}

#[cfg(test)]
mod tests {
    use super::ManagementFileConfig;

    #[test]
    fn management_file_defaults_use_host_relative_paths() {
        let config = ManagementFileConfig::default();
        assert_eq!(config.data_root, "sessions");
        assert_eq!(config.web_root, "web/dist");
    }
}
