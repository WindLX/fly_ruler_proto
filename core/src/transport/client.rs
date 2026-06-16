use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::logging::init_logging;
use crate::PROTOCOL_VERSION;
use crate::{pb, LoggingConfig};

use super::{now_secs, TransportError};

/// Low-level UDP client for protobuf transport.
pub struct Client {
    socket: UdpSocket,
    remote_addr: SocketAddr,
}

impl Client {
    fn make_request(command: pb::request_command::Kind, timestamp: Option<f64>) -> pb::Message {
        pb::Message {
            envelope: Some(pb::message::Envelope::Request(pb::Request {
                id: None,
                timestamp: timestamp.unwrap_or_else(now_secs),
                command: Some(pb::RequestCommand {
                    kind: Some(command),
                }),
            })),
        }
    }

    fn build_handshake_message(client_uuid: pb::Uuid) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::Handshake(pb::Handshake {
                version: PROTOCOL_VERSION.to_string(),
                client_uuid: Some(client_uuid),
            }),
            None,
        )
    }

    fn build_heartbeat_message(seq_num: u64, client_uuid: pb::Uuid) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::Heartbeat(pb::Heartbeat {
                seq_num,
                client_uuid: Some(client_uuid),
            }),
            None,
        )
    }

    fn build_spawn_message(
        aircraft_uuid: pb::Uuid,
        aircraft_name: String,
        toml_config: String,
        initial_state: pb::AircraftState,
    ) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::AircraftEvent(pb::AircraftEvent {
                aircraft_id: Some(aircraft_uuid),
                info: Some(pb::AircraftCommandInfo {
                    kind: Some(pb::aircraft_command_info::Kind::Spawn(
                        pb::AircraftSpawnInfo {
                            name: aircraft_name,
                            toml_config,
                            initial_state: Some(initial_state),
                        },
                    )),
                }),
            }),
            None,
        )
    }

    fn build_state_update_message(
        aircraft_uuid: pb::Uuid,
        state: pb::AircraftState,
        timestamp: Option<f64>,
    ) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::AircraftEvent(pb::AircraftEvent {
                aircraft_id: Some(aircraft_uuid),
                info: Some(pb::AircraftCommandInfo {
                    kind: Some(pb::aircraft_command_info::Kind::StateUpdate(state)),
                }),
            }),
            timestamp,
        )
    }

    fn build_custom_event_message(
        aircraft_uuid: pb::Uuid,
        event_name: String,
        timestamp: Option<f64>,
    ) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::AircraftEvent(pb::AircraftEvent {
                aircraft_id: Some(aircraft_uuid),
                info: Some(pb::AircraftCommandInfo {
                    kind: Some(pb::aircraft_command_info::Kind::CustomEvent(event_name)),
                }),
            }),
            timestamp,
        )
    }

    fn build_despawn_message(
        aircraft_uuid: pb::Uuid,
        reason: Option<String>,
        timestamp: Option<f64>,
    ) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::AircraftEvent(pb::AircraftEvent {
                aircraft_id: Some(aircraft_uuid),
                info: Some(pb::AircraftCommandInfo {
                    kind: Some(pb::aircraft_command_info::Kind::Despawn(pb::DespawnInfo {
                        reason,
                    })),
                }),
            }),
            timestamp,
        )
    }

    /// Connect to a remote endpoint over UDP.
    pub async fn connect(addr: &str, config: &LoggingConfig) -> Result<Self, TransportError> {
        init_logging(config);
        let remote_addr = addr
            .parse::<SocketAddr>()
            .map_err(|e| TransportError::InvalidMessage(format!("invalid socket addr: {e}")))?;
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(remote_addr).await?;
        info!(target: "fly_ruler_proto_core.transport", remote_addr = %remote_addr, "udp connection established");
        Ok(Self {
            socket,
            remote_addr,
        })
    }

    /// Send one protobuf message.
    pub async fn send(&mut self, msg: pb::Message) -> Result<(), TransportError> {
        let payload = prost::Message::encode_to_vec(&msg);
        self.socket.send(&payload).await?;
        debug!(target: "fly_ruler_proto_core.transport", remote_addr = %self.remote_addr, bytes = payload.len(), "datagram sent");
        Ok(())
    }

    /// Receive one protobuf message.
    pub async fn recv(&mut self) -> Result<Option<pb::Message>, TransportError> {
        let mut buf = vec![0_u8; 64 * 1024];
        let n = self.socket.recv(&mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        let msg = prost::Message::decode(&buf[..n])?;
        debug!(target: "fly_ruler_proto_core.transport", remote_addr = %self.remote_addr, bytes = n, "datagram received");
        Ok(Some(msg))
    }

    /// Get remote address.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Close connection.
    pub async fn close(&mut self) -> Result<(), TransportError> {
        info!(target: "fly_ruler_proto_core.transport", remote_addr = %self.remote_addr, "udp connection closed");
        Ok(())
    }
}

enum Outbound {
    Send(pb::Message),
    Shutdown,
}

enum Operation {
    UpdateState {
        state: pb::AircraftState,
        timestamp: Option<f64>,
    },
    CreateEvent {
        event_name: String,
        timestamp: Option<f64>,
    },
    Despawn {
        reason: Option<String>,
        timestamp: Option<f64>,
    },
    Stop,
}

fn uuid_to_pb(u: Uuid) -> pb::Uuid {
    pb::Uuid {
        value: u.as_bytes().to_vec(),
    }
}

/// High-level aircraft lifecycle client.
///
/// One instance represents one aircraft and manages:
/// - handshake + spawn bootstrap
/// - operation queue (update/event/despawn)
/// - heartbeat background task
pub struct AircraftClient {
    addr: String,
    client_uuid: Uuid,
    aircraft_uuid: Uuid,
    op_tx: Option<mpsc::UnboundedSender<Operation>>,
    heartbeat_stop_tx: Option<watch::Sender<bool>>,
    sender_handle: Option<JoinHandle<()>>,
    operation_handle: Option<JoinHandle<()>>,
    heartbeat_handle: Option<JoinHandle<()>>,
    closed: bool,
    despawned: bool,
}

impl AircraftClient {
    pub async fn connect(
        addr: &str,
        logging_config: &LoggingConfig,
        aircraft_name: String,
        initial_state: pb::AircraftState,
        toml_config: String,
        heartbeat_interval_secs: f64,
    ) -> Result<Self, TransportError> {
        let client_uuid = Uuid::new_v4();
        let aircraft_uuid = Uuid::new_v4();
        let client_uuid_for_handshake = client_uuid;
        let client_uuid_for_heartbeat = client_uuid;
        let aircraft_uuid_for_ops = aircraft_uuid;

        info!(
            target: "fly_ruler_proto_core.transport",
            addr = addr,
            client_uuid = %client_uuid,
            aircraft_uuid = %aircraft_uuid,
            "starting aircraft lifecycle client"
        );

        let rust_client = Client::connect(addr, logging_config).await?;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();
        let (op_tx, mut op_rx) = mpsc::unbounded_channel::<Operation>();
        let (heartbeat_stop_tx, heartbeat_stop_rx) = watch::channel(false);

        let sender_handle = tokio::spawn(async move {
            let mut client = rust_client;
            while let Some(outbound) = out_rx.recv().await {
                match outbound {
                    Outbound::Send(msg) => {
                        if let Err(e) = client.send(msg).await {
                            error!(target: "fly_ruler_proto_core.transport", error = %e, "client send failed");
                            break;
                        }
                    }
                    Outbound::Shutdown => {
                        info!(target: "fly_ruler_proto_core.transport", "client sender received shutdown");
                        break;
                    }
                }
            }
            if let Err(e) = client.close().await {
                warn!(target: "fly_ruler_proto_core.transport", error = %e, "client close returned error");
            }
            info!(target: "fly_ruler_proto_core.transport", "client sender task exited");
        });

        let _ = out_tx.send(Outbound::Send(Client::build_handshake_message(uuid_to_pb(
            client_uuid_for_handshake,
        ))));

        let op_out_tx = out_tx.clone();
        let operation_handle = tokio::spawn(async move {
            let _ = op_out_tx.send(Outbound::Send(Client::build_spawn_message(
                uuid_to_pb(aircraft_uuid_for_ops),
                aircraft_name,
                toml_config,
                initial_state,
            )));

            while let Some(op) = op_rx.recv().await {
                match op {
                    Operation::UpdateState { state, timestamp } => {
                        let _ = op_out_tx.send(Outbound::Send(Client::build_state_update_message(
                            uuid_to_pb(aircraft_uuid_for_ops),
                            state,
                            timestamp,
                        )));
                    }
                    Operation::CreateEvent {
                        event_name,
                        timestamp,
                    } => {
                        let _ = op_out_tx.send(Outbound::Send(Client::build_custom_event_message(
                            uuid_to_pb(aircraft_uuid_for_ops),
                            event_name,
                            timestamp,
                        )));
                    }
                    Operation::Despawn { reason, timestamp } => {
                        let _ = op_out_tx.send(Outbound::Send(Client::build_despawn_message(
                            uuid_to_pb(aircraft_uuid_for_ops),
                            reason,
                            timestamp,
                        )));
                    }
                    Operation::Stop => {
                        let _ = op_out_tx.send(Outbound::Shutdown);
                        break;
                    }
                }
            }
            info!(target: "fly_ruler_proto_core.transport", "client operation task exited");
        });

        let hb_out_tx = out_tx;
        let heartbeat_handle = tokio::spawn(async move {
            let interval = Duration::from_secs_f64(heartbeat_interval_secs.max(0.1));
            let mut ticker = tokio::time::interval(interval);
            let mut stop_rx = heartbeat_stop_rx;
            let mut seq_num: u64 = 0;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                        seq_num = seq_num.saturating_add(1);
                        let _ = hb_out_tx.send(Outbound::Send(Client::build_heartbeat_message(
                            seq_num,
                            uuid_to_pb(client_uuid_for_heartbeat),
                        )));
                        debug!(target: "fly_ruler_proto_core.transport", seq_num, "client heartbeat queued");
                    }
                    changed = stop_rx.changed() => {
                        if changed.is_err() || *stop_rx.borrow() {
                            break;
                        }
                    }
                }
            }
            info!(target: "fly_ruler_proto_core.transport", "client heartbeat task exited");
        });

        Ok(Self {
            addr: addr.to_string(),
            client_uuid,
            aircraft_uuid,
            op_tx: Some(op_tx),
            heartbeat_stop_tx: Some(heartbeat_stop_tx),
            sender_handle: Some(sender_handle),
            operation_handle: Some(operation_handle),
            heartbeat_handle: Some(heartbeat_handle),
            closed: false,
            despawned: false,
        })
    }

    pub fn client_uuid(&self) -> String {
        self.client_uuid.to_string()
    }

    pub fn aircraft_uuid(&self) -> String {
        self.aircraft_uuid.to_string()
    }

    pub fn update_state(
        &self,
        state: pb::AircraftState,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        self.send_operation(Operation::UpdateState { state, timestamp })
    }

    pub fn create_event(
        &self,
        event_name: String,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        self.send_operation(Operation::CreateEvent {
            event_name,
            timestamp,
        })
    }

    pub fn despawn(
        &mut self,
        reason: Option<String>,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        if self.despawned {
            return Ok(());
        }
        self.send_operation(Operation::Despawn { reason, timestamp })?;
        self.despawned = true;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }

        info!(
            target: "fly_ruler_proto_core.transport",
            addr = self.addr,
            client_uuid = %self.client_uuid,
            aircraft_uuid = %self.aircraft_uuid,
            "closing aircraft lifecycle client"
        );

        if !self.despawned {
            let _ = self.send_operation(Operation::Despawn {
                reason: Some("client_close".to_string()),
                timestamp: None,
            });
            self.despawned = true;
        }

        if let Some(stop_tx) = self.heartbeat_stop_tx.take() {
            let _ = stop_tx.send(true);
        }

        if let Some(op_tx) = self.op_tx.as_ref() {
            let _ = op_tx.send(Operation::Stop);
        }
        self.op_tx = None;

        if let Some(handle) = self.heartbeat_handle.take() {
            let _ = handle.await;
        }
        if let Some(handle) = self.operation_handle.take() {
            let _ = handle.await;
        }
        if let Some(handle) = self.sender_handle.take() {
            let _ = handle.await;
        }

        self.closed = true;
        Ok(())
    }

    fn ensure_open(&self) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::ClientChannelClosed("client is closed"));
        }
        Ok(())
    }

    fn send_operation(&self, op: Operation) -> Result<(), TransportError> {
        let tx = self
            .op_tx
            .as_ref()
            .ok_or(TransportError::ClientChannelClosed(
                "operation queue is closed",
            ))?;

        tx.send(op)
            .map_err(|_| TransportError::ClientChannelClosed("failed to enqueue operation"))
    }
}
