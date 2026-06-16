//! Core runtime configuration.
//!
//! This module centralizes configuration knobs used by kernel orchestration,
//! transport/session behavior and store ingestion behavior.

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
    /// Logging configuration.
    pub logging: LoggingConfig,
}
