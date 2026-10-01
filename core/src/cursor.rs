//! Cursor-aligned room snapshots and the receive-only UDP client.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use prost::Message as ProstMessage;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::pb;
use crate::playback::{PlaybackController, PlaybackMode, PlaybackSnapshot};
use crate::store::{Event, GlobalTimestampedEvent, TimeSeriesStore};
use crate::transport::ServerPublisherHandle;
use crate::transport::TransportError;
use crate::PROTOCOL_VERSION;

/// Maximum protobuf bytes carried by one UDP datagram payload fragment.
pub const CURSOR_CHUNK_BYTES: usize = 1_024;
/// Maximum accepted cursor payload after reassembly.
pub const MAX_CURSOR_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Maximum time an incomplete best-effort frame remains buffered.
pub const CURSOR_REASSEMBLY_TIMEOUT: Duration = Duration::from_millis(500);

/// Server-side cursor publication options.
#[derive(Debug, Clone)]
pub struct CursorStreamConfig {
    /// Snapshot publication frequency.
    pub publish_hz: f64,
    /// Maximum simultaneous cursor subscribers.
    pub max_subscribers: usize,
    /// Server-side live-state freshness threshold.
    pub stale_timeout: Duration,
    /// Reliable event retransmit interval.
    pub event_retry_interval: Duration,
    /// Maximum event retransmit attempts before a subscriber is removed.
    pub event_retry_limit: u32,
}

impl Default for CursorStreamConfig {
    fn default() -> Self {
        Self {
            publish_hz: 30.0,
            max_subscribers: 16,
            stale_timeout: Duration::from_millis(500),
            event_retry_interval: Duration::from_millis(250),
            event_retry_limit: 8,
        }
    }
}

/// Receive-only cursor client options.
#[derive(Debug, Clone)]
pub struct CursorClientConfig {
    /// Server UDP endpoint.
    pub server_address: String,
    /// Requested snapshot rate. The server may clamp this value.
    pub requested_hz: f64,
}

impl Default for CursorClientConfig {
    fn default() -> Self {
        Self {
            server_address: "127.0.0.1:18002".to_string(),
            requested_hz: 30.0,
        }
    }
}

/// One fully reassembled cursor stream item.
#[derive(Debug, Clone)]
pub enum CursorClientEvent {
    /// Latest complete best-effort world frame.
    Frame(pb::CursorFrame),
    /// Reliable ordered lifecycle/custom-event batch.
    EventBatch(pb::CursorEventBatch),
    /// Authoritative timeline reset.
    Reset(pb::CursorReset),
}

/// Read-only receive statistics for a cursor subscriber.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CursorClientStats {
    /// UDP datagrams accepted from the connected room endpoint.
    pub datagrams_received: u64,
    /// Complete, sequence-newer state frames published to the consumer.
    pub frames_received: u64,
    /// Complete state frames discarded because a newer frame was already seen.
    pub frames_dropped: u64,
    /// Fragmented payloads rejected or expired before completion.
    pub reassembly_failures: u64,
    /// Reliable event batches applied exactly once.
    pub event_batches_received: u64,
    /// Duplicate event batches received after an ACK was lost.
    pub event_retransmits: u64,
}

#[derive(Debug)]
struct PendingEvents {
    sequence: u64,
    revision: u64,
    cursor: Option<f64>,
    messages: Vec<pb::Message>,
    reset_messages: Vec<pb::Message>,
    last_sent: Instant,
    attempts: u32,
    final_page: bool,
}

#[derive(Debug, Default)]
struct SubscriberState {
    last_revision: Option<u64>,
    last_cursor: Option<f64>,
    next_event_sequence: u64,
    pending: VecDeque<PendingEvents>,
    last_frame_sent: Option<Instant>,
}

/// Background publisher that broadcasts the authoritative playback cursor.
pub struct CursorStreamRuntime {
    stop_token: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl CursorStreamRuntime {
    /// Start publishing cursor-aligned state and reliable events.
    pub fn start(
        server: ServerPublisherHandle,
        store: std::sync::Arc<TimeSeriesStore>,
        playback: std::sync::Arc<PlaybackController>,
        config: CursorStreamConfig,
        stale_timeout: Duration,
    ) -> Result<Self, TransportError> {
        if !config.publish_hz.is_finite() || config.publish_hz <= 0.0 {
            return Err(TransportError::InvalidMessage(
                "cursor publish_hz must be finite and greater than zero".to_string(),
            ));
        }
        if config.max_subscribers == 0 {
            return Err(TransportError::InvalidMessage(
                "cursor max_subscribers must be greater than zero".to_string(),
            ));
        }
        let stop_token = CancellationToken::new();
        let child = stop_token.child_token();
        let task = tokio::spawn(async move {
            let epoch = Uuid::new_v4();
            let mut states: HashMap<SocketAddr, SubscriberState> = HashMap::new();
            let mut frame_sequence = 0_u64;
            let mut interval =
                tokio::time::interval(Duration::from_secs_f64(1.0 / config.publish_hz));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = child.cancelled() => break,
                    _ = interval.tick() => {
                        let mut subscribers = server
                            .active_sessions()
                            .await
                            .into_iter()
                            .filter(|session| {
                                session.role == pb::ClientRole::CursorSubscriber
                                    && session.cursor_subscribed
                            })
                            .collect::<Vec<_>>();
                        subscribers.sort_by_key(|session| session.addr);
                        for overflow in subscribers.iter().skip(config.max_subscribers) {
                            server.remove_session(overflow.addr).await;
                        }
                        subscribers.truncate(config.max_subscribers);
                        let active: HashSet<_> = subscribers.iter().map(|value| value.addr).collect();
                        states.retain(|address, _| active.contains(address));
                        let snapshot = playback.snapshot();
                        frame_sequence = frame_sequence.wrapping_add(1);
                        for session in subscribers {
                            server
                                .set_cursor_epoch(session.addr, epoch.as_bytes().to_vec())
                                .await;
                            let state = states.entry(session.addr).or_default();
                            if let Some(completed) = state
                                .pending
                                .pop_front_if(|pending| session.cursor_event_ack >= pending.sequence)
                            {
                                if completed.final_page {
                                    state.last_revision = Some(completed.revision);
                                    state.last_cursor = completed.cursor;
                                }
                            }

                            if state.pending.is_empty() {
                                let revision_changed = state.last_revision != Some(snapshot.revision);
                                let cursor_reversed = matches!((state.last_cursor, snapshot.cursor_secs),
                                    (Some(previous), Some(current)) if current < previous);
                                let baseline = state.last_revision.is_none() || revision_changed || cursor_reversed;
                                let events = if baseline {
                                    baseline_events(&store, &playback, &snapshot)
                                } else {
                                    delta_events(&store, state.last_cursor, snapshot.cursor_secs)
                                };
                                if baseline || !events.is_empty() {
                                    let page_count = events.len().max(1).div_ceil(32);
                                    for page_index in 0..page_count {
                                        state.next_event_sequence =
                                            state.next_event_sequence.wrapping_add(1).max(1);
                                        let batch_sequence = state.next_event_sequence;
                                        let page_events = events
                                            .iter()
                                            .skip(page_index * 32)
                                            .take(32)
                                            .cloned()
                                            .enumerate()
                                            .map(|(index, event)| {
                                                cursor_event((batch_sequence << 32) | index as u64, event)
                                            })
                                            .collect();
                                        let batch = pb::CursorEventBatch {
                                            server_epoch: Some(uuid_to_pb(epoch)),
                                            event_sequence: batch_sequence,
                                            revision: snapshot.revision,
                                            baseline: baseline && page_index == 0,
                                            baseline_complete: page_index + 1 == page_count,
                                            events: page_events,
                                        };
                                        let messages = match encode_cursor_chunks(
                                            epoch,
                                            pb::CursorPayloadKind::EventBatch,
                                            batch_sequence,
                                            &batch,
                                        ) {
                                            Ok(value) => value,
                                            Err(_) => {
                                                state.pending.clear();
                                                break;
                                            }
                                        };
                                        let reset_messages = if baseline && page_index == 0 {
                                            let reset = pb::CursorReset {
                                                server_epoch: Some(uuid_to_pb(epoch)),
                                                revision: snapshot.revision,
                                                cursor: snapshot.cursor_secs,
                                                reason: if state.last_revision.is_none() {
                                                    "initial_sync".to_string()
                                                } else {
                                                    "timeline_revision".to_string()
                                                },
                                            };
                                            encode_cursor_chunks(
                                                epoch,
                                                pb::CursorPayloadKind::Reset,
                                                batch_sequence,
                                                &reset,
                                            )
                                            .unwrap_or_default()
                                        } else {
                                            Vec::new()
                                        };
                                        state.pending.push_back(PendingEvents {
                                            sequence: batch_sequence,
                                            revision: snapshot.revision,
                                            cursor: snapshot.cursor_secs,
                                            messages,
                                            reset_messages,
                                            last_sent: Instant::now(),
                                            attempts: 0,
                                            final_page: page_index + 1 == page_count,
                                        });
                                    }
                                } else {
                                    state.last_revision = Some(snapshot.revision);
                                    state.last_cursor = snapshot.cursor_secs;
                                }
                            }

                            if let Some(pending) = state.pending.front_mut() {
                                if pending.attempts == 0
                                    || pending.last_sent.elapsed() >= config.event_retry_interval
                                {
                                    if pending.attempts >= config.event_retry_limit {
                                        server.remove_session(session.addr).await;
                                        states.remove(&session.addr);
                                        continue;
                                    }
                                    for message in
                                        pending.reset_messages.iter().chain(&pending.messages)
                                    {
                                        let _ = server.send_to(message.clone(), session.addr).await;
                                    }
                                    pending.attempts += 1;
                                    pending.last_sent = Instant::now();
                                }
                            }

                            let watermark = state
                                .pending
                                .back()
                                .map_or(session.cursor_event_ack, |pending| pending.sequence);
                            let requested_hz = session.cursor_requested_hz.min(config.publish_hz);
                            let frame_interval = Duration::from_secs_f64(1.0 / requested_hz);
                            if state
                                .last_frame_sent
                                .is_some_and(|sent| sent.elapsed() < frame_interval)
                            {
                                continue;
                            }
                            let frame = assemble_cursor_frame(
                                &store,
                                &playback,
                                &snapshot,
                                stale_timeout,
                                epoch,
                                frame_sequence,
                                watermark,
                            );
                            if let Ok(messages) = encode_cursor_chunks(
                                epoch,
                                pb::CursorPayloadKind::Frame,
                                frame_sequence,
                                &frame,
                            ) {
                                for message in messages {
                                    let _ = server.send_to(message, session.addr).await;
                                }
                                state.last_frame_sent = Some(Instant::now());
                            }
                        }
                    }
                }
            }
        });
        Ok(Self {
            stop_token,
            task: Some(task),
        })
    }

    /// Stop publication and join the background task.
    pub async fn stop(&mut self) {
        self.stop_token.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

#[derive(Debug)]
struct PartialPayload {
    created_at: Instant,
    total_size: usize,
    checksum: u32,
    chunks: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Default)]
struct CursorReassembler {
    partial: HashMap<(i32, u64), PartialPayload>,
    expired_payloads: u64,
}

impl CursorReassembler {
    fn push(&mut self, chunk: pb::CursorChunk) -> Result<Option<Vec<u8>>, TransportError> {
        let before = self.partial.len();
        self.partial
            .retain(|_, value| value.created_at.elapsed() <= CURSOR_REASSEMBLY_TIMEOUT);
        self.expired_payloads = self
            .expired_payloads
            .saturating_add((before - self.partial.len()) as u64);
        let total_size = chunk.total_size as usize;
        let chunk_count = chunk.chunk_count as usize;
        let chunk_index = chunk.chunk_index as usize;
        if total_size == 0
            || total_size > MAX_CURSOR_PAYLOAD_BYTES
            || chunk_count == 0
            || chunk_count > MAX_CURSOR_PAYLOAD_BYTES.div_ceil(CURSOR_CHUNK_BYTES)
            || chunk_index >= chunk_count
        {
            return Err(TransportError::InvalidMessage(
                "invalid cursor chunk bounds".to_string(),
            ));
        }
        let key = (chunk.kind, chunk.sequence);
        let partial = self.partial.entry(key).or_insert_with(|| PartialPayload {
            created_at: Instant::now(),
            total_size,
            checksum: chunk.checksum,
            chunks: vec![None; chunk_count],
        });
        if partial.total_size != total_size
            || partial.checksum != chunk.checksum
            || partial.chunks.len() != chunk_count
        {
            self.partial.remove(&key);
            return Err(TransportError::InvalidMessage(
                "cursor chunk metadata changed during reassembly".to_string(),
            ));
        }
        partial.chunks[chunk_index] = Some(chunk.payload);
        if partial.chunks.iter().any(Option::is_none) {
            return Ok(None);
        }
        let mut payload = Vec::with_capacity(total_size);
        for bytes in &partial.chunks {
            payload.extend_from_slice(bytes.as_deref().unwrap_or_default());
        }
        self.partial.remove(&key);
        if payload.len() != total_size || checksum32(&payload) != chunk.checksum {
            return Err(TransportError::InvalidMessage(
                "cursor payload checksum mismatch".to_string(),
            ));
        }
        Ok(Some(payload))
    }
}

/// Assemble one atomic cursor frame from a playback snapshot.
pub fn assemble_cursor_frame(
    store: &TimeSeriesStore,
    playback: &PlaybackController,
    snapshot: &PlaybackSnapshot,
    stale_timeout: Duration,
    server_epoch: Uuid,
    frame_sequence: u64,
    event_watermark: u64,
) -> pb::CursorFrame {
    let configs: HashMap<_, _> = store
        .aircraft_summaries()
        .into_iter()
        .map(|summary| (summary.id, summary.config))
        .collect();
    let mut aircraft = Vec::new();
    for aircraft_id in store.get_aircraft_ids() {
        let Some(resolved) = playback.resolve_aircraft_with(snapshot, &aircraft_id) else {
            continue;
        };
        if !resolved.spawned {
            continue;
        }
        let config = configs.get(&aircraft_id).and_then(Option::as_ref);
        aircraft.push(pb::CursorAircraftSnapshot {
            aircraft_id: hex_to_pb_uuid(&aircraft_id),
            name: config.map_or_else(String::new, |value| value.name.clone()),
            toml_config: config.map_or_else(String::new, |value| value.toml_config.clone()),
            source_timestamp: resolved.sample.timestamp_secs,
            stale: snapshot.mode == PlaybackMode::Live
                && store.live_state_is_stale(&aircraft_id, stale_timeout),
            state: Some(resolved.sample.state),
        });
    }
    aircraft.sort_by(|left, right| {
        left.aircraft_id
            .as_ref()
            .map(|value| &value.value)
            .cmp(&right.aircraft_id.as_ref().map(|value| &value.value))
    });
    let (lower_bound, upper_bound) = snapshot
        .bounds
        .map_or((None, None), |(lower, upper)| (Some(lower), Some(upper)));
    pb::CursorFrame {
        server_epoch: Some(uuid_to_pb(server_epoch)),
        frame_sequence,
        mode: match snapshot.mode {
            PlaybackMode::Live => pb::CursorPlaybackMode::Live as i32,
            PlaybackMode::ReplayPaused => pb::CursorPlaybackMode::ReplayPaused as i32,
            PlaybackMode::ReplayPlaying => pb::CursorPlaybackMode::ReplayPlaying as i32,
        },
        cursor: snapshot.cursor_secs,
        lower_bound,
        upper_bound,
        speed: snapshot.speed,
        revision: snapshot.revision,
        event_watermark,
        generated_at: now_secs(),
        aircraft,
    }
}

/// Convert a stored event into its cursor wire representation.
pub fn cursor_event(sequence: u64, event: GlobalTimestampedEvent) -> pb::CursorEvent {
    let kind = match event.event {
        Event::Spawn(value) => pb::aircraft_command_info::Kind::Spawn(*value),
        Event::Despawn(value) => pb::aircraft_command_info::Kind::Despawn(value),
        Event::Custom(value) => pb::aircraft_command_info::Kind::CustomEvent(value),
    };
    pb::CursorEvent {
        sequence,
        aircraft_id: hex_to_pb_uuid(&event.aircraft_id),
        source_timestamp: event.timestamp_secs,
        info: Some(pb::AircraftCommandInfo { kind: Some(kind) }),
    }
}

/// Encode a cursor payload into MTU-safe protobuf push messages.
pub fn encode_cursor_chunks<M: ProstMessage>(
    epoch: Uuid,
    kind: pb::CursorPayloadKind,
    sequence: u64,
    value: &M,
) -> Result<Vec<pb::Message>, TransportError> {
    let payload = value.encode_to_vec();
    if payload.is_empty() || payload.len() > MAX_CURSOR_PAYLOAD_BYTES {
        return Err(TransportError::InvalidMessage(
            "cursor payload exceeds allowed size".to_string(),
        ));
    }
    let checksum = checksum32(&payload);
    let chunk_count = payload.len().div_ceil(CURSOR_CHUNK_BYTES);
    Ok(payload
        .chunks(CURSOR_CHUNK_BYTES)
        .enumerate()
        .map(|(chunk_index, bytes)| pb::Message {
            envelope: Some(pb::message::Envelope::ServerPush(pb::ServerPush {
                kind: Some(pb::server_push::Kind::CursorChunk(pb::CursorChunk {
                    server_epoch: Some(uuid_to_pb(epoch)),
                    kind: kind as i32,
                    sequence,
                    chunk_index: chunk_index as u32,
                    chunk_count: chunk_count as u32,
                    total_size: payload.len() as u32,
                    checksum,
                    payload: bytes.to_vec(),
                })),
            })),
        })
        .collect())
}

/// Receive-only UDP client for an authoritative cursor stream.
pub struct CursorClient {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    client_uuid: Uuid,
    heartbeat_sequence: u64,
    reassembler: CursorReassembler,
    stream_epoch: Vec<u8>,
    applied_event_sequence: u64,
    baseline_revision: Option<u64>,
    pending_frame: Option<pb::CursorFrame>,
    last_frame_sequence: u64,
    stats: CursorClientStats,
}

impl CursorClient {
    /// Connect, negotiate the subscriber role, and request cursor frames.
    pub async fn connect(config: &CursorClientConfig) -> Result<Self, TransportError> {
        if !config.requested_hz.is_finite() || config.requested_hz <= 0.0 {
            return Err(TransportError::InvalidMessage(
                "requested_hz must be finite and greater than zero".to_string(),
            ));
        }
        let remote_addr = config
            .server_address
            .parse::<SocketAddr>()
            .map_err(|error| TransportError::InvalidMessage(error.to_string()))?;
        let local_addr = if remote_addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(local_addr).await?;
        socket.connect(remote_addr).await?;
        let client_uuid = Uuid::new_v4();
        let mut client = Self {
            socket,
            remote_addr,
            client_uuid,
            heartbeat_sequence: 0,
            reassembler: CursorReassembler::default(),
            stream_epoch: Vec::new(),
            applied_event_sequence: 0,
            baseline_revision: None,
            pending_frame: None,
            last_frame_sequence: 0,
            stats: CursorClientStats::default(),
        };
        client
            .send_request(pb::request_command::Kind::Handshake(pb::Handshake {
                version: PROTOCOL_VERSION.to_string(),
                client_uuid: Some(uuid_to_pb(client_uuid)),
                role: pb::ClientRole::CursorSubscriber as i32,
            }))
            .await?;
        client.expect_ack().await?;
        client
            .send_request(pb::request_command::Kind::CursorSubscribe(
                pb::CursorSubscribe {
                    requested_hz: config.requested_hz,
                },
            ))
            .await?;
        client.expect_ack().await?;
        Ok(client)
    }

    /// Receive the next complete stream item, transparently reassembling chunks.
    pub async fn recv(&mut self) -> Result<CursorClientEvent, TransportError> {
        if self
            .pending_frame
            .as_ref()
            .is_some_and(|frame| frame.event_watermark <= self.applied_event_sequence)
        {
            let frame = self.pending_frame.take().expect("checked pending frame");
            if frame.frame_sequence > self.last_frame_sequence {
                self.last_frame_sequence = frame.frame_sequence;
                self.stats.frames_received = self.stats.frames_received.saturating_add(1);
                return Ok(CursorClientEvent::Frame(frame));
            }
            self.stats.frames_dropped = self.stats.frames_dropped.saturating_add(1);
        }
        loop {
            let mut buffer = vec![0_u8; 64 * 1024];
            let size = self.socket.recv(&mut buffer).await?;
            self.stats.datagrams_received = self.stats.datagrams_received.saturating_add(1);
            let message = pb::Message::decode(&buffer[..size])?;
            let Some(pb::message::Envelope::ServerPush(push)) = message.envelope else {
                continue;
            };
            let Some(pb::server_push::Kind::CursorChunk(chunk)) = push.kind else {
                continue;
            };
            let kind = pb::CursorPayloadKind::try_from(chunk.kind).map_err(|_| {
                TransportError::InvalidMessage("unknown cursor payload kind".to_string())
            })?;
            let expired_before = self.reassembler.expired_payloads;
            let reassembled = self.reassembler.push(chunk);
            self.stats.reassembly_failures = self.stats.reassembly_failures.saturating_add(
                self.reassembler
                    .expired_payloads
                    .saturating_sub(expired_before),
            );
            let Some(payload) = reassembled.inspect_err(|_| {
                self.stats.reassembly_failures = self.stats.reassembly_failures.saturating_add(1);
            })?
            else {
                continue;
            };
            return match kind {
                pb::CursorPayloadKind::Frame => {
                    let frame = pb::CursorFrame::decode(payload.as_slice())?;
                    if frame.frame_sequence <= self.last_frame_sequence {
                        self.stats.frames_dropped = self.stats.frames_dropped.saturating_add(1);
                        continue;
                    }
                    if frame.event_watermark <= self.applied_event_sequence {
                        self.last_frame_sequence = frame.frame_sequence;
                        self.stats.frames_received = self.stats.frames_received.saturating_add(1);
                        Ok(CursorClientEvent::Frame(frame))
                    } else {
                        let replace = self
                            .pending_frame
                            .as_ref()
                            .is_none_or(|current| frame.frame_sequence > current.frame_sequence);
                        if replace {
                            self.pending_frame = Some(frame);
                        }
                        continue;
                    }
                }
                pb::CursorPayloadKind::EventBatch => {
                    let batch = pb::CursorEventBatch::decode(payload.as_slice())?;
                    self.send_request(pb::request_command::Kind::CursorEventAck(
                        pb::CursorEventAck {
                            server_epoch: batch.server_epoch.clone(),
                            event_sequence: batch.event_sequence,
                        },
                    ))
                    .await?;
                    let epoch = batch
                        .server_epoch
                        .as_ref()
                        .map_or_else(Vec::new, |value| value.value.clone());
                    if self.stream_epoch != epoch {
                        self.stream_epoch = epoch;
                        self.applied_event_sequence = 0;
                        self.baseline_revision = None;
                        self.pending_frame = None;
                        self.last_frame_sequence = 0;
                    }
                    if batch.baseline && self.baseline_revision != Some(batch.revision) {
                        self.applied_event_sequence = 0;
                        self.baseline_revision = Some(batch.revision);
                        self.pending_frame = None;
                    }
                    if batch.event_sequence <= self.applied_event_sequence {
                        self.stats.event_retransmits =
                            self.stats.event_retransmits.saturating_add(1);
                        continue;
                    }
                    self.applied_event_sequence = batch.event_sequence;
                    self.stats.event_batches_received =
                        self.stats.event_batches_received.saturating_add(1);
                    Ok(CursorClientEvent::EventBatch(batch))
                }
                pb::CursorPayloadKind::Reset => {
                    let reset = pb::CursorReset::decode(payload.as_slice())?;
                    let epoch = reset
                        .server_epoch
                        .as_ref()
                        .map_or_else(Vec::new, |value| value.value.clone());
                    if self.stream_epoch == epoch && self.baseline_revision == Some(reset.revision)
                    {
                        continue;
                    }
                    self.stream_epoch = epoch;
                    self.applied_event_sequence = 0;
                    self.baseline_revision = Some(reset.revision);
                    self.pending_frame = None;
                    self.last_frame_sequence = 0;
                    Ok(CursorClientEvent::Reset(reset))
                }
                pb::CursorPayloadKind::Unspecified => Err(TransportError::InvalidMessage(
                    "unspecified cursor payload kind".to_string(),
                )),
            };
        }
    }

    /// Send one subscriber heartbeat.
    pub async fn heartbeat(&mut self) -> Result<(), TransportError> {
        self.heartbeat_sequence = self.heartbeat_sequence.wrapping_add(1);
        self.send_request(pb::request_command::Kind::Heartbeat(pb::Heartbeat {
            seq_num: self.heartbeat_sequence,
            client_uuid: Some(uuid_to_pb(self.client_uuid)),
        }))
        .await
    }

    /// Return the connected room address.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Return the ephemeral local UDP endpoint.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.socket.local_addr()?)
    }

    /// Return a copy of the current receive statistics.
    pub fn stats(&self) -> CursorClientStats {
        self.stats
    }

    async fn send_request(
        &mut self,
        kind: pb::request_command::Kind,
    ) -> Result<(), TransportError> {
        let message = pb::Message {
            envelope: Some(pb::message::Envelope::Request(pb::Request {
                id: None,
                timestamp: now_secs(),
                command: Some(pb::RequestCommand { kind: Some(kind) }),
            })),
        };
        self.socket.send(&message.encode_to_vec()).await?;
        Ok(())
    }

    async fn expect_ack(&mut self) -> Result<(), TransportError> {
        let mut buffer = vec![0_u8; 64 * 1024];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let size = tokio::time::timeout_at(deadline, self.socket.recv(&mut buffer))
                .await
                .map_err(|_| TransportError::HandshakeTimeout)??;
            let message = pb::Message::decode(&buffer[..size])?;
            let Some(pb::message::Envelope::Response(response)) = message.envelope else {
                // A publisher can observe a completed subscription before its
                // ACK datagram reaches this socket. Reliable baseline data is
                // retransmitted, so it is safe to ignore that early push here.
                continue;
            };
            return match response.result {
                Some(pb::response::Result::Ok(pb::ResponseData {
                    kind: Some(pb::response_data::Kind::Ack(true)),
                })) => Ok(()),
                Some(pb::response::Result::Err(error)) => {
                    Err(TransportError::HandshakeRejected(error.message))
                }
                _ => Err(TransportError::InvalidMessage(
                    "cursor subscription was not acknowledged".to_string(),
                )),
            };
        }
    }
}

fn uuid_to_pb(value: Uuid) -> pb::Uuid {
    pb::Uuid {
        value: value.as_bytes().to_vec(),
    }
}

fn hex_to_pb_uuid(value: &str) -> Option<pb::Uuid> {
    let decoded = Uuid::parse_str(value).ok().or_else(|| {
        if value.len() == 32 {
            Uuid::parse_str(&format!(
                "{}-{}-{}-{}-{}",
                &value[0..8],
                &value[8..12],
                &value[12..16],
                &value[16..20],
                &value[20..32]
            ))
            .ok()
        } else {
            None
        }
    })?;
    Some(uuid_to_pb(decoded))
}

fn checksum32(payload: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in payload {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

fn baseline_events(
    store: &TimeSeriesStore,
    playback: &PlaybackController,
    snapshot: &PlaybackSnapshot,
) -> Vec<GlobalTimestampedEvent> {
    let Some(cursor) = snapshot.cursor_secs else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for aircraft_id in store.get_aircraft_ids() {
        let Some(resolved) = playback.resolve_aircraft_with(snapshot, &aircraft_id) else {
            continue;
        };
        if !resolved.spawned {
            continue;
        }
        let Some(events) = store.get_events_range(&aircraft_id, f64::NEG_INFINITY, cursor) else {
            continue;
        };
        let first = events
            .iter()
            .rposition(|entry| matches!(entry.event, Event::Spawn(_)))
            .unwrap_or(0);
        output.extend(
            events
                .into_iter()
                .skip(first)
                .map(|entry| GlobalTimestampedEvent {
                    aircraft_id: aircraft_id.clone(),
                    timestamp_secs: entry.timestamp_secs,
                    event: entry.event,
                }),
        );
    }
    output.sort_by(|left, right| {
        left.timestamp_secs
            .total_cmp(&right.timestamp_secs)
            .then_with(|| left.aircraft_id.cmp(&right.aircraft_id))
    });
    output
}

fn delta_events(
    store: &TimeSeriesStore,
    previous_cursor: Option<f64>,
    cursor: Option<f64>,
) -> Vec<GlobalTimestampedEvent> {
    let (Some(previous), Some(current)) = (previous_cursor, cursor) else {
        return Vec::new();
    };
    if current < previous {
        return Vec::new();
    }
    let page = store.get_global_events_page(previous, current, 0, usize::MAX);
    page.items
        .into_iter()
        .filter(|entry| entry.timestamp_secs > previous)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Event, KernelRuntime, RuntimeConfig};

    #[test]
    fn chunks_reassemble_out_of_order_and_reject_corruption() {
        let epoch = Uuid::new_v4();
        let frame = pb::CursorFrame {
            server_epoch: Some(uuid_to_pb(epoch)),
            frame_sequence: 7,
            aircraft: (0..30)
                .map(|index| pb::CursorAircraftSnapshot {
                    name: "x".repeat(index + 80),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut messages = encode_cursor_chunks(
            epoch,
            pb::CursorPayloadKind::Frame,
            frame.frame_sequence,
            &frame,
        )
        .unwrap();
        assert!(messages.len() > 1);
        messages.reverse();
        let mut reassembler = CursorReassembler::default();
        let mut decoded = None;
        for message in messages {
            let Some(pb::message::Envelope::ServerPush(push)) = message.envelope else {
                panic!("expected push");
            };
            let Some(pb::server_push::Kind::CursorChunk(chunk)) = push.kind else {
                panic!("expected chunk");
            };
            if let Some(payload) = reassembler.push(chunk).unwrap() {
                decoded = Some(pb::CursorFrame::decode(payload.as_slice()).unwrap());
            }
        }
        assert_eq!(decoded.unwrap().frame_sequence, 7);
    }

    #[test]
    fn checksum_detects_mutation() {
        assert_ne!(checksum32(b"frame-a"), checksum32(b"frame-b"));
    }

    #[test]
    fn reassembly_expires_incomplete_payloads_and_rejects_corruption() {
        let epoch = Uuid::new_v4();
        let frame = pb::CursorFrame {
            server_epoch: Some(uuid_to_pb(epoch)),
            frame_sequence: 9,
            aircraft: (0..20)
                .map(|_| pb::CursorAircraftSnapshot {
                    name: "payload".repeat(64),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let messages = encode_cursor_chunks(
            epoch,
            pb::CursorPayloadKind::Frame,
            frame.frame_sequence,
            &frame,
        )
        .unwrap();
        let mut chunks = messages
            .into_iter()
            .map(|message| {
                let Some(pb::message::Envelope::ServerPush(push)) = message.envelope else {
                    panic!("server push")
                };
                let Some(pb::server_push::Kind::CursorChunk(chunk)) = push.kind else {
                    panic!("cursor chunk")
                };
                chunk
            })
            .collect::<Vec<_>>();
        assert!(chunks.len() > 1);

        let mut expired = CursorReassembler::default();
        assert!(expired.push(chunks[0].clone()).unwrap().is_none());
        expired.partial.values_mut().for_each(|value| {
            value.created_at = Instant::now() - CURSOR_REASSEMBLY_TIMEOUT - Duration::from_millis(1)
        });
        let mut next_sequence = chunks[0].clone();
        next_sequence.sequence += 1;
        assert!(expired.push(next_sequence).unwrap().is_none());
        assert_eq!(expired.expired_payloads, 1);

        chunks.last_mut().unwrap().payload[0] ^= 0xff;
        let mut corrupted = CursorReassembler::default();
        let mut error = None;
        for chunk in chunks {
            if let Err(value) = corrupted.push(chunk) {
                error = Some(value);
                break;
            }
        }
        assert!(matches!(error, Some(TransportError::InvalidMessage(_))));
    }

    #[tokio::test]
    async fn two_cursor_clients_receive_the_same_baseline_and_frame() {
        let store = Arc::new(TimeSeriesStore::new());
        let aircraft_id = Uuid::new_v4().simple().to_string();
        store.append_event(
            aircraft_id.clone(),
            1.0,
            Event::Spawn(Box::new(pb::AircraftSpawnInfo {
                name: "room-aircraft".to_string(),
                ..Default::default()
            })),
        );
        store.append_state(
            aircraft_id.clone(),
            2.0,
            pb::AircraftState {
                position: Some(pb::Vector3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }),
                attitude: Some(pb::Quaternion {
                    w: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        for index in 0..40 {
            store.append_event(
                aircraft_id.clone(),
                2.1 + f64::from(index) * 0.01,
                Event::Custom(format!("baseline-{index}")),
            );
        }
        let mut kernel = KernelRuntime::new(Arc::clone(&store));
        kernel.start_server("127.0.0.1:0").await.unwrap();
        let address = kernel.udp_local_addr().unwrap().to_string();
        let mut client = CursorClient::connect(&CursorClientConfig {
            server_address: address.clone(),
            requested_hz: 30.0,
        })
        .await
        .unwrap();
        let mut second_client = CursorClient::connect(&CursorClientConfig {
            server_address: address,
            requested_hz: 20.0,
        })
        .await
        .unwrap();

        async fn receive_baseline_and_frame(client: &mut CursorClient) -> (usize, pb::CursorFrame) {
            let mut baseline_pages = 0;
            for _ in 0..8 {
                let event = tokio::time::timeout(Duration::from_secs(1), client.recv())
                    .await
                    .unwrap()
                    .unwrap();
                match event {
                    CursorClientEvent::EventBatch(batch) => {
                        baseline_pages += 1;
                        assert_eq!(batch.baseline_complete, baseline_pages == 2);
                    }
                    CursorClientEvent::Frame(frame) => return (baseline_pages, frame),
                    CursorClientEvent::Reset(_) => {}
                }
            }
            panic!("cursor frame")
        }

        let (first_baseline_pages, first_frame) = receive_baseline_and_frame(&mut client).await;
        let (second_baseline_pages, second_frame) =
            receive_baseline_and_frame(&mut second_client).await;
        assert_eq!(first_baseline_pages, 2);
        assert_eq!(second_baseline_pages, 2);
        assert_eq!(first_frame.revision, second_frame.revision);
        assert_eq!(first_frame.cursor, second_frame.cursor);
        assert_eq!(first_frame.aircraft.len(), 1);
        assert_eq!(first_frame.aircraft[0].name, "room-aircraft");
        assert_eq!(client.stats().frames_received, 1);
        assert_eq!(second_client.stats().frames_received, 1);
        kernel.stop_server().await;
    }

    #[tokio::test]
    async fn unacknowledged_event_batch_is_retransmitted_with_the_same_sequence() {
        let store = Arc::new(TimeSeriesStore::new());
        let aircraft_id = Uuid::new_v4().simple().to_string();
        store.append_event(aircraft_id.clone(), 1.0, Event::Spawn(Box::default()));
        store.append_state(
            aircraft_id,
            1.1,
            pb::AircraftState {
                position: Some(pb::Vector3::default()),
                attitude: Some(pb::Quaternion {
                    w: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        let config = RuntimeConfig {
            cursor_stream: CursorStreamConfig {
                event_retry_interval: Duration::from_millis(30),
                event_retry_limit: 4,
                ..CursorStreamConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut kernel = KernelRuntime::with_config(Arc::clone(&store), config);
        kernel.start_server("127.0.0.1:0").await.unwrap();
        let mut client = CursorClient::connect(&CursorClientConfig {
            server_address: kernel.udp_local_addr().unwrap().to_string(),
            requested_hz: 30.0,
        })
        .await
        .unwrap();
        let mut reassembler = CursorReassembler::default();

        async fn receive_event_sequence(
            client: &mut CursorClient,
            reassembler: &mut CursorReassembler,
        ) -> u64 {
            loop {
                let mut buffer = vec![0_u8; 64 * 1024];
                let size =
                    tokio::time::timeout(Duration::from_secs(1), client.socket.recv(&mut buffer))
                        .await
                        .expect("event retry timeout")
                        .unwrap();
                let message = pb::Message::decode(&buffer[..size]).unwrap();
                let Some(pb::message::Envelope::ServerPush(push)) = message.envelope else {
                    continue;
                };
                let Some(pb::server_push::Kind::CursorChunk(chunk)) = push.kind else {
                    continue;
                };
                if chunk.kind != pb::CursorPayloadKind::EventBatch as i32 {
                    continue;
                }
                let Some(payload) = reassembler.push(chunk).unwrap() else {
                    continue;
                };
                return pb::CursorEventBatch::decode(payload.as_slice())
                    .unwrap()
                    .event_sequence;
            }
        }

        let first = receive_event_sequence(&mut client, &mut reassembler).await;
        let retransmitted = receive_event_sequence(&mut client, &mut reassembler).await;
        assert_eq!(first, retransmitted);
        kernel.stop_server().await;
    }
}
