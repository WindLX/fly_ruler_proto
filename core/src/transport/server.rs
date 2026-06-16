use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::config::TransportConfig;
use crate::pb;
use crate::utils::{now_secs, uuid_to_hex};
use crate::PROTOCOL_VERSION;

use super::TransportError;

/// Session metadata for a connected client.
#[derive(Debug, Clone)]
pub struct Session {
    /// Remote socket address.
    pub addr: SocketAddr,
    /// Client UUID as a lowercase hex string.
    pub client_uuid_hex: String,
    /// Last seen timestamp in seconds since the Unix epoch.
    pub last_seen_secs: f64,
}

impl Session {
    /// Create a new session for the given address and client UUID.
    pub fn new(addr: SocketAddr, client_uuid_hex: String) -> Self {
        Self {
            addr,
            client_uuid_hex,
            last_seen_secs: now_secs(),
        }
    }

    /// Return true if the session has been inactive longer than `timeout_secs`.
    pub fn is_expired(&self, timeout_secs: f64) -> bool {
        let now = now_secs();
        self.last_seen_secs + timeout_secs < now
    }
}

fn req_timestamp(req: &pb::Request) -> f64 {
    if req.timestamp.is_finite() {
        req.timestamp
    } else {
        now_secs()
    }
}

fn make_ack_for_request(req: &pb::Request) -> pb::Message {
    let req_id = req.id.clone();
    pb::Message {
        envelope: Some(pb::message::Envelope::Response(pb::Response {
            id: req_id,
            timestamp: req_timestamp(req),
            result: Some(pb::response::Result::Ok(pb::ResponseData {
                kind: Some(pb::response_data::Kind::Ack(true)),
            })),
        })),
    }
}

fn make_err_for_request(req: &pb::Request, code: pb::ErrorCode, message: &str) -> pb::Message {
    let req_id = req.id.clone();
    pb::Message {
        envelope: Some(pb::message::Envelope::Response(pb::Response {
            id: req_id,
            timestamp: req_timestamp(req),
            result: Some(pb::response::Result::Err(pb::ResponseError {
                code: code as i32,
                message: message.to_string(),
                aircraft_id: None,
            })),
        })),
    }
}

#[derive(Clone)]
struct SessionState {
    by_addr: Arc<Mutex<HashMap<SocketAddr, Session>>>,
    by_client_uuid: Arc<Mutex<HashMap<String, SocketAddr>>>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            by_addr: Arc::new(Mutex::new(HashMap::new())),
            by_client_uuid: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn touch_addr(&self, addr: SocketAddr) {
        let mut by_addr = self.by_addr.lock().await;
        if let Some(s) = by_addr.get_mut(&addr) {
            s.last_seen_secs = now_secs();
        }
    }

    async fn set_session(&self, addr: SocketAddr, client_uuid: String) {
        let mut by_addr = self.by_addr.lock().await;
        let mut by_client_uuid = self.by_client_uuid.lock().await;

        if let Some(old_addr) = by_client_uuid.insert(client_uuid.clone(), addr) {
            by_addr.remove(&old_addr);
        }

        by_addr.insert(
            addr,
            Session {
                addr,
                client_uuid_hex: client_uuid,
                last_seen_secs: now_secs(),
            },
        );
    }

    async fn remove_by_addr(&self, addr: SocketAddr) {
        let mut by_addr = self.by_addr.lock().await;
        let mut by_client_uuid = self.by_client_uuid.lock().await;
        if let Some(old) = by_addr.remove(&addr) {
            by_client_uuid.remove(&old.client_uuid_hex);
        }
    }

    async fn list(&self) -> Vec<Session> {
        let by_addr = self.by_addr.lock().await;
        by_addr.values().cloned().collect()
    }

    async fn cleanup_expired(&self, timeout: Duration) {
        let deadline = now_secs() - timeout.as_secs_f64();

        let expired: Vec<(SocketAddr, String)> = {
            let by_addr = self.by_addr.lock().await;
            by_addr
                .iter()
                .filter_map(|(addr, sess)| {
                    if sess.last_seen_secs < deadline {
                        Some((*addr, sess.client_uuid_hex.clone()))
                    } else {
                        None
                    }
                })
                .collect()
        };

        if expired.is_empty() {
            return;
        }

        let mut by_addr = self.by_addr.lock().await;
        let mut by_client_uuid = self.by_client_uuid.lock().await;
        for (addr, client_uuid) in expired {
            by_addr.remove(&addr);
            by_client_uuid.remove(&client_uuid);
            info!(
                target: "fly_ruler_proto_core.transport",
                addr = %addr,
                client_uuid = client_uuid,
                "session expired and removed"
            );
        }
    }
}

/// Low-level UDP server with session bookkeeping.
pub struct Server {
    socket: Arc<UdpSocket>,
    sessions: SessionState,
    timeout: Duration,
}

impl Server {
    /// Bind a UDP socket to the given address.
    pub async fn bind(addr: &str, config: TransportConfig) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(addr).await?;
        let local_addr = socket.local_addr()?;
        info!(target: "fly_ruler_proto_core.transport", local_addr = %local_addr, "udp server bound");
        Ok(Self {
            socket: Arc::new(socket),
            sessions: SessionState::new(),
            timeout: Duration::from_secs(config.heartbeat_timeout_secs.max(1)),
        })
    }

    /// Receive one message from the socket, cleaning up expired sessions first.
    pub async fn recv_from(
        &self,
    ) -> Result<Option<(pb::Message, SocketAddr, Option<String>)>, TransportError> {
        self.sessions.cleanup_expired(self.timeout).await;

        let mut buf = vec![0_u8; 64 * 1024];
        let (size, addr) = self.socket.recv_from(&mut buf).await?;
        if size == 0 {
            return Ok(None);
        }

        let msg: pb::Message = prost::Message::decode(&buf[..size])?;
        let client_uuid = msg
            .envelope
            .as_ref()
            .and_then(|env| match env {
                pb::message::Envelope::Request(req) => {
                    req.command.as_ref().and_then(|cmd| cmd.kind.as_ref())
                }
                _ => None,
            })
            .and_then(|kind| match kind {
                pb::request_command::Kind::Handshake(hs) => {
                    hs.client_uuid.as_ref().map(uuid_to_hex)
                }
                pb::request_command::Kind::Heartbeat(hb) => {
                    hb.client_uuid.as_ref().map(uuid_to_hex)
                }
                _ => None,
            });

        self.sessions.touch_addr(addr).await;

        Ok(Some((msg, addr, client_uuid)))
    }

    /// Send a protobuf message to the given address.
    pub async fn send_to(&self, msg: pb::Message, addr: SocketAddr) -> Result<(), TransportError> {
        let payload = prost::Message::encode_to_vec(&msg);
        self.socket.send_to(&payload, addr).await?;
        debug!(target: "fly_ruler_proto_core.transport", addr = %addr, bytes = payload.len(), "datagram sent to client");
        Ok(())
    }

    /// Close the server socket.
    pub async fn close(&self) -> Result<(), TransportError> {
        let local_addr = self.socket.local_addr()?;
        info!(target: "fly_ruler_proto_core.transport", local_addr = %local_addr, "udp server closed");
        Ok(())
    }

    /// Return the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.socket.local_addr()?)
    }

    /// Register or replace a session for the given address and client UUID.
    pub async fn set_session(&self, addr: SocketAddr, client_uuid: String) {
        self.sessions.set_session(addr, client_uuid).await;
    }

    /// Remove the session associated with the given address.
    pub async fn remove_session(&self, addr: SocketAddr) {
        self.sessions.remove_by_addr(addr).await;
    }

    /// Return the list of currently active sessions.
    pub async fn active_sessions(&self) -> Vec<Session> {
        self.sessions.list().await
    }
}

/// Long-running UDP server runtime with automatic handshake/heartbeat ACK.
///
/// The `handler` callback is invoked synchronously from the async receive loop
/// for each non-handshake/heartbeat message. It must not block; offload heavy
/// work to a separate task if necessary.
pub struct ServerRuntime {
    server: Arc<Server>,
    stop_token: CancellationToken,
    recv_handle: Option<JoinHandle<()>>,
}

impl ServerRuntime {
    /// Start a server runtime on the given address.
    ///
    /// `handler` is called for each aircraft event message. It must be
    /// non-blocking; any synchronous I/O or heavy computation will stall the
    /// UDP receive loop.
    pub async fn start<F>(
        addr: &str,
        config: &TransportConfig,
        handler: F,
    ) -> Result<Self, TransportError>
    where
        F: Fn(pb::Message, SocketAddr) + Send + Sync + 'static,
    {
        let server = Arc::new(Server::bind(addr, config.clone()).await?);
        let stop_token = CancellationToken::new();
        let stop_child_token = stop_token.child_token();
        let handler: Arc<dyn Fn(pb::Message, SocketAddr) + Send + Sync + 'static> =
            Arc::new(handler);

        let recv_server = Arc::clone(&server);
        let recv_handler = Arc::clone(&handler);
        let recv_handle = tokio::spawn(async move {
            let stop_child_token = stop_child_token;
            let mut pending_events: VecDeque<(pb::Message, SocketAddr)> = VecDeque::new();

            loop {
                tokio::select! {
                    _ = stop_child_token.cancelled() => {
                        info!(target: "fly_ruler_proto_core.transport", "server runtime received stop signal");
                        break;
                    }
                    received = recv_server.recv_from() => {
                        match received {
                            Ok(Some((msg, addr, client_uuid))) => {
                                let mut ack_to_send = None;
                                match &msg.envelope {
                                    Some(pb::message::Envelope::Request(req)) => {
                                        if let Some(cmd) = req.command.as_ref().and_then(|c| c.kind.as_ref()) {
                                            match cmd {
                                                pb::request_command::Kind::Handshake(hs) => {
                                                    let valid_version = hs.version == PROTOCOL_VERSION;
                                                    if valid_version {
                                                        if let Some(uuid) = client_uuid {
                                                            recv_server.set_session(addr, uuid).await;
                                                        }
                                                        ack_to_send = Some(make_ack_for_request(req));
                                                    } else {
                                                        ack_to_send = Some(make_err_for_request(
                                                            req,
                                                            pb::ErrorCode::ProtocolVersionMismatch,
                                                            "protocol version mismatch",
                                                        ));
                                                    }
                                                }
                                                pb::request_command::Kind::Heartbeat(_) => {
                                                    ack_to_send = Some(make_ack_for_request(req));
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Some(pb::message::Envelope::Response(_)) => {
                                        // Server side ignores inbound responses from clients.
                                    }
                                    None => {
                                        warn!(target: "fly_ruler_proto_core.transport", addr = %addr, "received message with empty envelope");
                                    }
                                }

                                if let Some(ack) = ack_to_send {
                                    if let Err(e) = recv_server.send_to(ack, addr).await {
                                        warn!(target: "fly_ruler_proto_core.transport", addr = %addr, error = %e, "failed to send server ack");
                                    }
                                }

                                pending_events.push_back((msg, addr));
                                while let Some((ev, source)) = pending_events.pop_front() {
                                    (recv_handler)(ev, source);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                error!(target: "fly_ruler_proto_core.transport", error = %e, "server receive loop terminated by error");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            server,
            stop_token,
            recv_handle: Some(recv_handle),
        })
    }

    /// Stop the server runtime and close the socket.
    pub async fn stop(&mut self) -> Result<(), TransportError> {
        self.stop_token.cancel();
        if let Some(handle) = self.recv_handle.take() {
            let _ = handle.await;
        }
        self.server.close().await
    }

    /// Return the list of currently active sessions.
    pub async fn active_sessions(&self) -> Vec<Session> {
        self.server.active_sessions().await
    }

    /// Return the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.server.local_addr()
    }
}
