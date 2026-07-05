//! HTTP/WebSocket management API and persistence operation orchestration.

/// Ingestion gate for pausing/resuming message intake.
pub mod gate;
/// HTTP/WebSocket route handlers for the management API.
pub mod routes;
/// Series catalog and query helpers used by the management API.
pub mod series;
/// Management server runtime and shared application state.
pub mod server;
/// Workspace snapshot storage used by the management API.
pub mod workspace;

pub use gate::IngestionGate;
pub use server::{ManagementError, ManagementServerRuntime, OperationRecord, OperationState};
