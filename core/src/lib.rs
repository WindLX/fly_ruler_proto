//! Fly Ruler Protocol Kernel core library.
//!
//! Provides the UDP/protobuf wire protocol, transport runtime, time-series
//! store, and kernel orchestration used by the Python and Godot bindings.

#![warn(missing_docs)]
#![deny(unsafe_code)]

/// Validated aircraft attitude and rotation operations.
pub mod attitude;
/// Runtime configuration types.
pub mod config;
/// Kernel orchestration and server lifecycle.
pub mod kernel;
/// Tracing subscriber initialization.
pub mod logging;
/// HTTP/WebSocket management API.
pub mod management;
/// Generated protobuf types.
pub mod pb;
/// Shared live/replay timeline controller.
pub mod playback;
/// Time-series storage and persistence.
pub mod store;
/// UDP transport runtime.
pub mod transport;
pub(crate) mod utils;

/// Protocol semantic version shared across core and language bindings.
pub const PROTOCOL_VERSION: &str = "0.3.0";

// Re-export commonly used types
pub use attitude::{Attitude, AttitudeError};
pub use config::{
    LoggingConfig, LoggingFileConfig, ManagementConfig, ManagementFileConfig, PlaybackFileConfig,
    ReplayConfig, RuntimeConfig, RuntimeFileConfig, StoreConfig, TransportConfig,
    TransportFileConfig, RUNTIME_CONFIG_SCHEMA_VERSION,
};
pub use kernel::{KernelRuntime, RuntimeError};
pub use logging::init_logging;
pub use management::{
    IngestionGate, ManagementError, ManagementServerRuntime, OperationRecord, OperationState,
};
pub use playback::{
    PlaybackController, PlaybackError, PlaybackMode, PlaybackSnapshot, PlaybackStepDirection,
    PlaybackStepUnit, ResolvedState,
};
pub use store::{
    AircraftConfig, AircraftId, AircraftSummary, AircraftTimeSeries, Event, StoreError, StorePage,
    TimeSeriesStore, TimestampedEvent, TimestampedState, TimestampedTelemetryFrame,
};
pub use transport::{
    AircraftClient, Client, Server, ServerRuntime, Session, SessionHandle, TransportError,
};
