use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::logging::init_logging;
use crate::pb;
use crate::utils::now_secs;
use crate::LoggingConfig;
use crate::PROTOCOL_VERSION;

use super::TransportError;

fn validate_source_timestamp(timestamp: Option<f64>) -> Result<(), TransportError> {
    if timestamp.is_some_and(|value| !value.is_finite()) {
        return Err(TransportError::InvalidMessage(
            "source timestamp must be finite".to_string(),
        ));
    }
    Ok(())
}

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
        telemetry_schemas: Vec<pb::TelemetryStreamSchema>,
        timestamp: Option<f64>,
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
                            telemetry_schemas,
                        },
                    )),
                }),
            }),
            timestamp,
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

    fn build_telemetry_frame_message(
        aircraft_uuid: pb::Uuid,
        frame: pb::TelemetryFrame,
        timestamp: Option<f64>,
    ) -> pb::Message {
        Self::make_request(
            pb::request_command::Kind::AircraftEvent(pb::AircraftEvent {
                aircraft_id: Some(aircraft_uuid),
                info: Some(pb::AircraftCommandInfo {
                    kind: Some(pb::aircraft_command_info::Kind::TelemetryFrame(frame)),
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

/// Outbound message or control signal for the sender task.
enum Outbound {
    /// Send a protobuf message to the remote endpoint.
    Send(Box<pb::Message>),
    /// Stop the sender task.
    Shutdown,
}

/// Operations queued by user-facing methods and processed by the operation task.
enum Operation {
    /// Update the aircraft state.
    UpdateState {
        /// New aircraft state.
        state: Box<pb::AircraftState>,
        /// Optional explicit timestamp; `None` uses the current time.
        timestamp: Option<f64>,
    },
    /// Create a custom event.
    CreateEvent {
        /// Event name.
        event_name: String,
        /// Optional explicit timestamp.
        timestamp: Option<f64>,
    },
    /// Publish one schema-validated telemetry frame.
    PublishTelemetry {
        /// Stream values and sequence.
        frame: Box<pb::TelemetryFrame>,
        /// Optional explicit timestamp.
        timestamp: Option<f64>,
    },
    /// Despawn the aircraft.
    Despawn {
        /// Optional reason string.
        reason: Option<String>,
        /// Optional explicit timestamp.
        timestamp: Option<f64>,
    },
    /// Stop the operation task.
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
    /// Connect to a remote server and start the aircraft lifecycle.
    ///
    /// This sends a `Handshake` and `Spawn` message automatically.
    pub async fn connect(
        addr: &str,
        logging_config: &LoggingConfig,
        aircraft_name: String,
        initial_state: pb::AircraftState,
        toml_config: String,
        heartbeat_interval_secs: f64,
    ) -> Result<Self, TransportError> {
        Self::connect_with_telemetry(
            addr,
            logging_config,
            aircraft_name,
            initial_state,
            toml_config,
            heartbeat_interval_secs,
            Vec::new(),
        )
        .await
    }

    /// Connect and register immutable telemetry stream schemas at spawn.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_with_telemetry(
        addr: &str,
        logging_config: &LoggingConfig,
        aircraft_name: String,
        initial_state: pb::AircraftState,
        toml_config: String,
        heartbeat_interval_secs: f64,
        telemetry_schemas: Vec<pb::TelemetryStreamSchema>,
    ) -> Result<Self, TransportError> {
        Self::connect_with_telemetry_at(
            addr,
            logging_config,
            aircraft_name,
            initial_state,
            toml_config,
            heartbeat_interval_secs,
            telemetry_schemas,
            None,
        )
        .await
    }

    /// Connect, register telemetry schemas, and place spawn on an explicit time base.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_with_telemetry_at(
        addr: &str,
        logging_config: &LoggingConfig,
        aircraft_name: String,
        initial_state: pb::AircraftState,
        toml_config: String,
        heartbeat_interval_secs: f64,
        telemetry_schemas: Vec<pb::TelemetryStreamSchema>,
        spawn_timestamp: Option<f64>,
    ) -> Result<Self, TransportError> {
        validate_source_timestamp(spawn_timestamp)?;
        if !heartbeat_interval_secs.is_finite() || heartbeat_interval_secs <= 0.0 {
            return Err(TransportError::InvalidMessage(
                "heartbeat interval must be finite and greater than zero".to_string(),
            ));
        }
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
                        if let Err(e) = client.send(*msg).await {
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

        send_outbound(
            &out_tx,
            Outbound::Send(Box::new(Client::build_handshake_message(uuid_to_pb(
                client_uuid_for_handshake,
            )))),
            "handshake",
        );

        let op_out_tx = out_tx.clone();
        let operation_handle = tokio::spawn(async move {
            send_outbound(
                &op_out_tx,
                Outbound::Send(Box::new(Client::build_spawn_message(
                    uuid_to_pb(aircraft_uuid_for_ops),
                    aircraft_name,
                    toml_config,
                    initial_state,
                    telemetry_schemas,
                    spawn_timestamp,
                ))),
                "spawn",
            );

            while let Some(op) = op_rx.recv().await {
                match op {
                    Operation::UpdateState { state, timestamp } => {
                        send_outbound(
                            &op_out_tx,
                            Outbound::Send(Box::new(Client::build_state_update_message(
                                uuid_to_pb(aircraft_uuid_for_ops),
                                *state,
                                timestamp,
                            ))),
                            "state_update",
                        );
                    }
                    Operation::CreateEvent {
                        event_name,
                        timestamp,
                    } => {
                        send_outbound(
                            &op_out_tx,
                            Outbound::Send(Box::new(Client::build_custom_event_message(
                                uuid_to_pb(aircraft_uuid_for_ops),
                                event_name,
                                timestamp,
                            ))),
                            "custom_event",
                        );
                    }
                    Operation::PublishTelemetry { frame, timestamp } => {
                        send_outbound(
                            &op_out_tx,
                            Outbound::Send(Box::new(Client::build_telemetry_frame_message(
                                uuid_to_pb(aircraft_uuid_for_ops),
                                *frame,
                                timestamp,
                            ))),
                            "telemetry_frame",
                        );
                    }
                    Operation::Despawn { reason, timestamp } => {
                        send_outbound(
                            &op_out_tx,
                            Outbound::Send(Box::new(Client::build_despawn_message(
                                uuid_to_pb(aircraft_uuid_for_ops),
                                reason,
                                timestamp,
                            ))),
                            "despawn",
                        );
                    }
                    Operation::Stop => {
                        send_outbound(&op_out_tx, Outbound::Shutdown, "shutdown");
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
                        send_outbound(
                            &hb_out_tx,
                            Outbound::Send(Box::new(Client::build_heartbeat_message(
                                seq_num,
                                uuid_to_pb(client_uuid_for_heartbeat),
                            ))),
                            "heartbeat",
                        );
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

    /// Return the client UUID as a string.
    pub fn client_uuid(&self) -> String {
        self.client_uuid.to_string()
    }

    /// Return the aircraft UUID as a string.
    pub fn aircraft_uuid(&self) -> String {
        self.aircraft_uuid.to_string()
    }

    /// Enqueue a state update for the aircraft.
    pub fn update_state(
        &self,
        state: pb::AircraftState,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        validate_source_timestamp(timestamp)?;
        self.send_operation(Operation::UpdateState {
            state: Box::new(state),
            timestamp,
        })
    }

    /// Enqueue a custom event for the aircraft.
    pub fn create_event(
        &self,
        event_name: String,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        validate_source_timestamp(timestamp)?;
        self.send_operation(Operation::CreateEvent {
            event_name,
            timestamp,
        })
    }

    /// Enqueue a telemetry frame for the aircraft.
    pub fn publish_telemetry(
        &self,
        frame: pb::TelemetryFrame,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        validate_source_timestamp(timestamp)?;
        self.send_operation(Operation::PublishTelemetry {
            frame: Box::new(frame),
            timestamp,
        })
    }

    /// Enqueue a despawn command for the aircraft.
    pub fn despawn(
        &mut self,
        reason: Option<String>,
        timestamp: Option<f64>,
    ) -> Result<(), TransportError> {
        self.ensure_open()?;
        if self.despawned {
            return Ok(());
        }
        validate_source_timestamp(timestamp)?;
        self.send_operation(Operation::Despawn { reason, timestamp })?;
        self.despawned = true;
        Ok(())
    }

    /// Close the client, sending a despawn if needed and stopping background tasks.
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
            if let Err(e) = self.send_operation(Operation::Despawn {
                reason: Some("client_close".to_string()),
                timestamp: None,
            }) {
                warn!(target: "fly_ruler_proto_core.transport", error = %e, "failed to enqueue close-despawn");
            }
            self.despawned = true;
        }

        if let Some(stop_tx) = self.heartbeat_stop_tx.take() {
            if stop_tx.send(true).is_err() {
                warn!(target: "fly_ruler_proto_core.transport", "heartbeat stop channel already closed");
            }
        }

        if let Some(op_tx) = self.op_tx.as_ref() {
            if op_tx.send(Operation::Stop).is_err() {
                warn!(target: "fly_ruler_proto_core.transport", "operation stop channel already closed");
            }
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

/// Send an outbound message, logging a warning if the channel has closed.
fn send_outbound(tx: &mpsc::UnboundedSender<Outbound>, outbound: Outbound, kind: &str) {
    if let Err(e) = tx.send(outbound) {
        warn!(
            target: "fly_ruler_proto_core.transport",
            kind = kind,
            error = %e,
            "failed to enqueue outbound message; sender task may have exited"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_timestamps_accept_simulation_time_and_reject_non_finite_values() {
        assert!(validate_source_timestamp(Some(0.0)).is_ok());
        assert!(validate_source_timestamp(Some(-12.5)).is_ok());
        assert!(validate_source_timestamp(None).is_ok());
        assert!(validate_source_timestamp(Some(f64::NAN)).is_err());
        assert!(validate_source_timestamp(Some(f64::INFINITY)).is_err());
    }
}
