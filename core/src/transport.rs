//! Network transport layer for Fly Ruler protocol.
//!
//! This module provides UDP client/server abstractions using Tokio and `pb::Message`.
//! Session-like state is maintained at the application layer via
//! handshake/heartbeat messages that carry a `client_uuid`.

use thiserror::Error;

/// UDP client implementations.
pub mod client;
/// UDP server runtime and session management.
pub mod server;

pub use client::{AircraftClient, Client};
pub use server::{Server, ServerRuntime, Session, SessionHandle};

/// Transport errors.
#[derive(Debug, Error)]
pub enum TransportError {
    /// IO error from the underlying UDP socket.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Protobuf decode error.
    #[error("decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// Invalid or unsupported message in transport state machine.
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// Client has not established a valid session.
    #[error("client is not registered by handshake/heartbeat")]
    UnregisteredClient,

    /// Client-side internal channel closed unexpectedly.
    #[error("client channel closed: {0}")]
    ClientChannelClosed(&'static str),
}
