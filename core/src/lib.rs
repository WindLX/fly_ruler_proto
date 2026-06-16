//! Fly Ruler Protocol Kernel
//!
//! A high-performance binary serialization protocol library for aerospace flight simulation.
//! Located between Python SDK (sender) and Godot Server (receiver), enabling 1000Hz+
//! state updates with microsecond-level serialization latency.
//!
//! ## Protocol Format
//! - Protobuf datagram payload over UDP
//! - Transport: Tokio UDP with app-layer session state (handshake/heartbeat)
//! - Serialization: `prost` / protobuf
//! - Protocol version constant: [`PROTOCOL_VERSION`]
//!
//! ## Usage
//! ```rust
//! use fly_ruler_proto_core::{pb, Codec};
//! use tokio::net::UdpSocket;
//!
//! let _msg = pb::Message {
//!     envelope: Some(pb::message::Envelope::Request(pb::Request {
//!         id: None,
//!         timestamp: 0.0,
//!         command: Some(pb::RequestCommand {
//!             kind: Some(pb::request_command::Kind::Heartbeat(pb::Heartbeat {
//!                 seq_num: 1,
//!                 client_uuid: None,
//!             })),
//!         }),
//!     })),
//! };
//! ```
//!
//! ## Module Structure
//! - [`pb`] - Generated protobuf message types
//! - [`codec`] - Legacy length-delimited codec utilities
//! - [`transport`] - UDP Client/Server abstractions
//! - [`store`] - In-memory time-series store and explicit persistence
//! - [`kernel`] - Runtime orchestration helpers

pub mod config;
pub mod kernel;
pub mod logging;
pub mod pb;
pub mod store;
pub mod transport;

/// Protocol semantic version shared across core and language bindings.
pub const PROTOCOL_VERSION: &str = "1.0.0";

// Re-export commonly used types
pub use config::{LoggingConfig, RuntimeConfig, StoreConfig, TransportConfig};
pub use kernel::{KernelRuntime, RuntimeError};
pub use logging::init_logging;
pub use store::{
    AircraftId, AircraftTimeSeries, Event, StoreError, TimeSeriesStore, TimestampedEvent,
    TimestampedState,
};
pub use transport::{AircraftClient, Client, Server, ServerRuntime, Session, TransportError};
