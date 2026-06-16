//! Public API orchestration layer.
//!
//! This module contains runtime/server/session lifecycle coordination and uses
//! the store module as the state backend.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use thiserror::Error;
use tracing::{info, warn};

use crate::config::RuntimeConfig;
use crate::logging::init_logging;
use crate::store::{StoreError, TimeSeriesStore};
use crate::transport::{ServerRuntime, Session, TransportError};

/// Errors that can occur when using the kernel runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// An error originating from the transport layer.
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),

    /// An error originating from the store layer.
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// Orchestrates a UDP server runtime and a shared time-series store.
///
/// This is the primary embedded entry point for Godot and other consumers
/// that need to receive, persist, and query protocol messages.
pub struct KernelRuntime {
    store: Arc<TimeSeriesStore>,
    config: RuntimeConfig,
    udp_runtime: Option<ServerRuntime>,
}

impl KernelRuntime {
    /// Create a new kernel runtime with default configuration.
    pub fn new(store: Arc<TimeSeriesStore>) -> Self {
        Self::with_config(store, RuntimeConfig::default())
    }

    /// Create a new kernel runtime with the provided configuration.
    pub fn with_config(store: Arc<TimeSeriesStore>, config: RuntimeConfig) -> Self {
        init_logging(&config.logging);
        info!(target: "fly_ruler_proto_core.runtime", "kernel runtime initialized");
        Self {
            store,
            config,
            udp_runtime: None,
        }
    }

    /// Return a reference to the runtime configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Return a clone of the shared store handle.
    pub fn store(&self) -> Arc<TimeSeriesStore> {
        Arc::clone(&self.store)
    }

    /// Start the UDP server runtime on the given address.
    ///
    /// If a server is already running, it is stopped and replaced.
    pub async fn start_server(&mut self, addr: &str) -> Result<(), RuntimeError> {
        if self.udp_runtime.is_some() {
            self.stop_server().await;
        }

        info!(target: "fly_ruler_proto_core.runtime", addr = addr, "starting UDP server runtime");
        let store = Arc::clone(&self.store);
        let store_config = self.config.store.clone();
        let runtime = ServerRuntime::start(addr, &self.config.transport, move |msg, _from| {
            store.append_message_with_config(msg, &store_config);
        })
        .await?;

        self.udp_runtime = Some(runtime);
        info!(target: "fly_ruler_proto_core.runtime", addr = addr, "UDP server runtime started");
        Ok(())
    }

    /// Stop the UDP server runtime, if one is running.
    pub async fn stop_server(&mut self) {
        info!(target: "fly_ruler_proto_core.runtime", "stopping UDP server runtime");

        if let Some(mut runtime) = self.udp_runtime.take() {
            if let Err(e) = runtime.stop().await {
                warn!(target: "fly_ruler_proto_core.runtime", error = %e, "error stopping UDP server runtime");
            }
        }
    }

    /// Return the list of currently active sessions.
    pub async fn active_sessions(&self) -> Vec<Session> {
        match &self.udp_runtime {
            Some(runtime) => runtime.active_sessions().await,
            None => Vec::new(),
        }
    }

    /// Return the local socket address of the running UDP server.
    ///
    /// Returns an error if no server is currently running.
    pub fn udp_local_addr(&self) -> Result<SocketAddr, RuntimeError> {
        let Some(runtime) = &self.udp_runtime else {
            return Err(RuntimeError::Transport(TransportError::InvalidMessage(
                "udp server is not running".to_string(),
            )));
        };
        Ok(runtime.local_addr()?)
    }

    /// Persist the current in-memory session to disk.
    pub fn save_session(&self, path: &Path) -> Result<(), RuntimeError> {
        info!(target: "fly_ruler_proto_core.runtime", path = %path.display(), "saving runtime session");
        self.store.save_to_disk(path)?;
        Ok(())
    }

    /// Load a session snapshot from disk, replacing current in-memory contents.
    pub fn load_session(&self, path: &Path) -> Result<(), RuntimeError> {
        info!(target: "fly_ruler_proto_core.runtime", path = %path.display(), "loading runtime session");
        self.store.load_from_disk(path)?;
        Ok(())
    }

    /// Clear all in-memory session data.
    pub fn clear_session(&self) {
        self.store.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb;
    use crate::transport::Client;

    fn handshake_message() -> pb::Message {
        pb::Message {
            envelope: Some(pb::message::Envelope::Request(pb::Request {
                id: Some(pb::Uuid {
                    value: vec![0x11; 16],
                }),
                timestamp: 42.0,
                command: Some(pb::RequestCommand {
                    kind: Some(pb::request_command::Kind::Handshake(pb::Handshake {
                        version: crate::PROTOCOL_VERSION.to_string(),
                        client_uuid: Some(pb::Uuid {
                            value: vec![0x22; 16],
                        }),
                    })),
                }),
            })),
        }
    }

    #[tokio::test]
    async fn runtime_can_start_and_stop_server() {
        let store = Arc::new(TimeSeriesStore::new());
        let mut runtime = KernelRuntime::new(store);
        runtime.start_server("127.0.0.1:0").await.unwrap();
        assert_eq!(runtime.active_sessions().await.len(), 0);
        runtime.stop_server().await;
    }

    #[tokio::test]
    async fn runtime_replies_ack_for_handshake() {
        let store = Arc::new(TimeSeriesStore::new());
        let mut runtime = KernelRuntime::new(store);
        runtime.start_server("127.0.0.1:0").await.unwrap();
        let addr = runtime.udp_local_addr().unwrap().to_string();

        let mut client = Client::connect(&addr, &crate::LoggingConfig::default())
            .await
            .unwrap();
        client.send(handshake_message()).await.unwrap();

        let ack = client.recv().await.unwrap().unwrap();
        let Some(pb::message::Envelope::Response(resp)) = ack.envelope else {
            panic!("expected response envelope");
        };
        assert!(matches!(
            resp.result,
            Some(pb::response::Result::Ok(pb::ResponseData {
                kind: Some(pb::response_data::Kind::Ack(true))
            }))
        ));

        let sessions = runtime.active_sessions().await;
        assert_eq!(sessions.len(), 1);
        runtime.stop_server().await;
    }

    #[tokio::test]
    async fn restarting_udp_server_replaces_old_runtime_state() {
        let store = Arc::new(TimeSeriesStore::new());
        let mut runtime = KernelRuntime::new(store);
        runtime.start_server("127.0.0.1:0").await.unwrap();
        let first_addr = runtime.udp_local_addr().unwrap();

        runtime.stop_server().await;
        runtime.start_server("127.0.0.1:0").await.unwrap();
        let second_addr = runtime.udp_local_addr().unwrap();

        assert_ne!(first_addr.port(), second_addr.port());
        runtime.stop_server().await;
    }
}
