//! Network transport layer for Fly Ruler protocol.
//!
//! This module provides UDP client/server abstractions using Tokio and `pb::Message`.
//! Session-like state is maintained at the application layer via
//! handshake/heartbeat messages that carry a `client_uuid`.

use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

pub mod client;
pub mod server;

pub use client::{AircraftClient, Client};
pub use server::{Session, Server, ServerRuntime};

/// Transport errors.
#[derive(Debug, Error)]
pub enum TransportError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Decode error
    #[error("decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// Invalid or unsupported message in transport state machine
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// Client has not established a valid session
    #[error("client is not registered by handshake/heartbeat")]
    UnregisteredClient,

    /// Client-side internal channel closed unexpectedly.
    #[error("client channel closed: {0}")]
    ClientChannelClosed(&'static str),
}

pub(crate) fn uuid_to_hex(uuid: &crate::pb::Uuid) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(uuid.value.len() * 2);
    for b in &uuid.value {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn now_secs() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(v) => v.as_secs_f64(),
        Err(_) => 0.0,
    }
}
