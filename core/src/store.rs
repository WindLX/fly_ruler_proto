//! State store and persistence layer.
//!
//! Responsibilities in this module:
//! - Trajectory level: append + query only (no trajectory delete/modify APIs)
//! - Aircraft level: create/read/update/delete and per-aircraft settings
//! - Async persistence and restore

use std::fs;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{Array, BinaryArray, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use dashmap::DashMap;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::config::StoreConfig;
use crate::pb;
use crate::utils::uuid_to_hex;
use crate::PROTOCOL_VERSION;

/// Identifier for an aircraft, represented as a lowercase hex UUID string.
pub type AircraftId = String;

/// Configuration metadata persisted for a spawned aircraft.
#[derive(Debug, Clone)]
pub struct AircraftConfig {
    /// Aircraft display name.
    pub name: String,
    /// Raw TOML configuration string.
    pub toml_config: String,
}

/// Lifecycle events that can be stored for an aircraft.
#[derive(Debug, Clone)]
pub enum Event {
    /// Aircraft spawn event, including initial configuration.
    Spawn(Box<pb::AircraftSpawnInfo>),
    /// Aircraft despawn event.
    Despawn(pb::DespawnInfo),
    /// Custom named event.
    Custom(String),
}

/// A single aircraft state sample with its timestamp.
#[derive(Debug, Clone)]
pub struct TimestampedState {
    /// Timestamp in seconds since the Unix epoch.
    pub timestamp_secs: f64,
    /// Aircraft state at this timestamp.
    pub state: pb::AircraftState,
}

/// A single aircraft event with its timestamp.
#[derive(Debug, Clone)]
pub struct TimestampedEvent {
    /// Timestamp in seconds since the Unix epoch.
    pub timestamp_secs: f64,
    /// Event that occurred at this timestamp.
    pub event: Event,
}

/// Time-series data for one aircraft.
#[derive(Debug, Clone, Default)]
pub struct AircraftTimeSeries {
    /// State samples, kept sorted by timestamp.
    pub states: Vec<TimestampedState>,
    /// Lifecycle events, kept sorted by timestamp.
    pub events: Vec<TimestampedEvent>,
    /// Configuration set by the first `Spawn` event.
    pub config: Option<AircraftConfig>,
}

/// In-memory time-series store for aircraft states and events.
#[derive(Debug, Default)]
pub struct TimeSeriesStore {
    data: DashMap<AircraftId, AircraftTimeSeries>,
}

/// Errors that can occur when persisting or loading store data.
#[derive(Debug, Error)]
pub enum StoreError {
    /// IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Parquet read/write error.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Arrow conversion error.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Protobuf decode error.
    #[error("decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// Invalid or inconsistent data.
    #[error("invalid data: {0}")]
    InvalidData(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct MetaFile {
    version: String,
    aircrafts: Vec<MetaAircraft>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetaAircraft {
    id: String,
    name: Option<String>,
    toml_config: Option<String>,
    time_range: Option<(f64, f64)>,
    state_count: usize,
    event_count: usize,
}

impl TimeSeriesStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
        }
    }

    /// Append a state sample for the given aircraft.
    ///
    /// States are kept sorted by timestamp.
    pub fn append_state(&self, id: AircraftId, timestamp: f64, state: pb::AircraftState) {
        let mut series = self.data.entry(id).or_default();
        if series
            .states
            .last()
            .is_none_or(|last| last.timestamp_secs <= timestamp)
        {
            series.states.push(TimestampedState {
                timestamp_secs: timestamp,
                state,
            });
            return;
        }

        let insert_at = series
            .states
            .binary_search_by(|s| s.timestamp_secs.total_cmp(&timestamp))
            .unwrap_or_else(|idx| idx);
        series.states.insert(
            insert_at,
            TimestampedState {
                timestamp_secs: timestamp,
                state,
            },
        );
    }

    /// Append an event for the given aircraft.
    ///
    /// Events are kept sorted by timestamp. A `Spawn` event sets the aircraft
    /// configuration.
    pub fn append_event(&self, id: AircraftId, timestamp: f64, event: Event) {
        let mut series = self.data.entry(id).or_default();
        if let Event::Spawn(spawn) = &event {
            series.config = Some(AircraftConfig {
                name: spawn.name.clone(),
                toml_config: spawn.toml_config.clone(),
            });
        }

        if series
            .events
            .last()
            .is_none_or(|last| last.timestamp_secs <= timestamp)
        {
            series.events.push(TimestampedEvent {
                timestamp_secs: timestamp,
                event,
            });
            return;
        }

        let insert_at = series
            .events
            .binary_search_by(|e| e.timestamp_secs.total_cmp(&timestamp))
            .unwrap_or_else(|idx| idx);
        series.events.insert(
            insert_at,
            TimestampedEvent {
                timestamp_secs: timestamp,
                event,
            },
        );
    }

    /// Append a protobuf message to the store using default store config.
    pub fn append_message(&self, msg: pb::Message) {
        self.append_message_with_config(msg, &StoreConfig);
    }

    /// Append a protobuf message to the store with the given store config.
    ///
    /// `config` is currently a placeholder for future store-level knobs.
    pub fn append_message_with_config(
        &self,
        msg: pb::Message,
        #[allow(unused_variables)] config: &StoreConfig,
    ) {
        let Some(pb::message::Envelope::Request(req)) = msg.envelope else {
            debug!(target: "fly_ruler_proto_core.store", "ignored non-request envelope");
            return;
        };

        let timestamp = req.timestamp;
        let Some(command) = req.command.and_then(|c| c.kind) else {
            warn!(target: "fly_ruler_proto_core.store", "ignored request without command");
            return;
        };

        let pb::request_command::Kind::AircraftEvent(event) = command else {
            debug!(target: "fly_ruler_proto_core.store", "ignored non-aircraft command");
            return;
        };

        let aircraft_id = event
            .aircraft_id
            .as_ref()
            .map(uuid_to_hex)
            .unwrap_or_else(|| "unknown".to_string());

        let Some(info) = event.info.and_then(|i| i.kind) else {
            warn!(target: "fly_ruler_proto_core.store", aircraft_id = aircraft_id, "ignored aircraft event without command info");
            return;
        };

        match info {
            pb::aircraft_command_info::Kind::StateUpdate(state) => {
                self.append_state(aircraft_id, timestamp, state);
            }
            pb::aircraft_command_info::Kind::Spawn(spawn) => {
                if let Some(state) = spawn.initial_state.clone() {
                    self.append_state(aircraft_id.clone(), timestamp, state);
                }
                self.append_event(aircraft_id, timestamp, Event::Spawn(Box::new(spawn)));
            }
            pb::aircraft_command_info::Kind::Despawn(despawn) => {
                self.append_event(aircraft_id, timestamp, Event::Despawn(despawn));
            }
            pb::aircraft_command_info::Kind::CustomEvent(custom) => {
                self.append_event(aircraft_id, timestamp, Event::Custom(custom));
            }
        }
    }

    /// Return the latest state sample for an aircraft, if any.
    pub fn get_latest(&self, id: &AircraftId) -> Option<TimestampedState> {
        self.data
            .get(id)
            .and_then(|series| series.states.last().cloned())
    }

    /// Return all state samples for an aircraft within the inclusive time range.
    pub fn get_states_range(
        &self,
        id: &AircraftId,
        start: f64,
        end: f64,
    ) -> Option<Vec<TimestampedState>> {
        self.data.get(id).map(|series| {
            series
                .states
                .iter()
                .filter(|s| s.timestamp_secs >= start && s.timestamp_secs <= end)
                .cloned()
                .collect()
        })
    }

    /// Return all events for an aircraft within the inclusive time range.
    pub fn get_events_range(
        &self,
        id: &AircraftId,
        start: f64,
        end: f64,
    ) -> Option<Vec<TimestampedEvent>> {
        self.data.get(id).map(|series| {
            series
                .events
                .iter()
                .filter(|e| e.timestamp_secs >= start && e.timestamp_secs <= end)
                .cloned()
                .collect()
        })
    }

    /// Return all aircraft IDs currently in the store, sorted.
    pub fn get_aircraft_ids(&self) -> Vec<AircraftId> {
        let mut ids: Vec<_> = self.data.iter().map(|item| item.key().clone()).collect();
        ids.sort();
        ids
    }

    /// Remove all in-memory data.
    pub fn clear(&self) {
        self.data.clear();
    }

    /// Persist the store to disk at the given directory path.
    pub fn save_to_disk(&self, path: &Path) -> Result<(), StoreError> {
        info!(target: "fly_ruler_proto_core.store", path = %path.display(), "saving store to disk");
        fs::create_dir_all(path)?;

        self.write_meta(path)?;
        self.write_states_parquet(path)?;
        self.write_events_parquet(path)?;
        info!(target: "fly_ruler_proto_core.store", path = %path.display(), aircraft_count = self.get_aircraft_ids().len(), "store save completed");
        Ok(())
    }

    /// Load a store snapshot from disk, replacing current in-memory contents.
    pub fn load_from_disk(&self, path: &Path) -> Result<(), StoreError> {
        info!(target: "fly_ruler_proto_core.store", path = %path.display(), "loading store from disk");
        self.clear();

        self.read_states_parquet(path)?;
        self.read_events_parquet(path)?;
        self.read_meta(path)?;
        info!(target: "fly_ruler_proto_core.store", path = %path.display(), aircraft_count = self.get_aircraft_ids().len(), "store load completed");
        Ok(())
    }

    fn write_meta(&self, path: &Path) -> Result<(), StoreError> {
        let mut aircrafts = Vec::new();
        for item in &self.data {
            let id = item.key().clone();
            let series = item.value();
            let time_range = series
                .states
                .first()
                .zip(series.states.last())
                .map(|(a, b)| (a.timestamp_secs, b.timestamp_secs));

            aircrafts.push(MetaAircraft {
                id,
                name: series.config.as_ref().map(|c| c.name.clone()),
                toml_config: series.config.as_ref().map(|c| c.toml_config.clone()),
                time_range,
                state_count: series.states.len(),
                event_count: series.events.len(),
            });
        }

        aircrafts.sort_by(|a, b| a.id.cmp(&b.id));
        let meta = MetaFile {
            version: PROTOCOL_VERSION.to_string(),
            aircrafts,
        };

        let bytes = serde_json::to_vec_pretty(&meta)?;
        fs::write(path.join("meta.json"), bytes)?;
        Ok(())
    }

    fn read_meta(&self, path: &Path) -> Result<(), StoreError> {
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            return Ok(());
        }

        let data = fs::read(meta_path)?;
        let meta: MetaFile = serde_json::from_slice(&data)?;
        for entry in meta.aircrafts {
            if let Some(mut series) = self.data.get_mut(&entry.id) {
                if let (Some(name), Some(toml_config)) = (entry.name, entry.toml_config) {
                    series.config = Some(AircraftConfig { name, toml_config });
                }
            }
        }

        Ok(())
    }

    fn write_states_parquet(&self, path: &Path) -> Result<(), StoreError> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("aircraft_id", DataType::Utf8, false),
            Field::new("timestamp", DataType::Float64, false),
            Field::new("state_payload", DataType::Binary, false),
        ]));

        let mut ids = Vec::<String>::new();
        let mut ts = Vec::<f64>::new();
        let mut payloads = Vec::<Vec<u8>>::new();

        for item in &self.data {
            let id = item.key().clone();
            for state in &item.value().states {
                ids.push(id.clone());
                ts.push(state.timestamp_secs);
                payloads.push(state.state.encode_to_vec());
            }
        }

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(ids)) as Arc<dyn Array>,
                Arc::new(Float64Array::from(ts)) as Arc<dyn Array>,
                Arc::new(BinaryArray::from_iter_values(payloads)) as Arc<dyn Array>,
            ],
        )?;

        let file = File::create(path.join("states.parquet"))?;
        let mut writer = ArrowWriter::try_new(file, schema, None)?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    }

    fn write_events_parquet(&self, path: &Path) -> Result<(), StoreError> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("aircraft_id", DataType::Utf8, false),
            Field::new("timestamp", DataType::Float64, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("event_payload", DataType::Binary, false),
        ]));

        let mut ids = Vec::<String>::new();
        let mut ts = Vec::<f64>::new();
        let mut kinds = Vec::<String>::new();
        let mut payloads = Vec::<Vec<u8>>::new();

        for item in &self.data {
            let id = item.key().clone();
            for event in &item.value().events {
                ids.push(id.clone());
                ts.push(event.timestamp_secs);
                let (kind, payload) = encode_event(&event.event);
                kinds.push(kind);
                payloads.push(payload);
            }
        }

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(ids)) as Arc<dyn Array>,
                Arc::new(Float64Array::from(ts)) as Arc<dyn Array>,
                Arc::new(StringArray::from(kinds)) as Arc<dyn Array>,
                Arc::new(BinaryArray::from_iter_values(payloads)) as Arc<dyn Array>,
            ],
        )?;

        let file = File::create(path.join("events.parquet"))?;
        let mut writer = ArrowWriter::try_new(file, schema, None)?;
        writer.write(&batch)?;
        writer.close()?;
        Ok(())
    }

    fn read_states_parquet(&self, path: &Path) -> Result<(), StoreError> {
        let file_path = path.join("states.parquet");
        if !file_path.exists() {
            return Ok(());
        }

        let file = File::open(file_path)?;
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)?
            .with_batch_size(1024)
            .build()?;

        for maybe_batch in &mut reader {
            let batch = maybe_batch?;
            let ids = as_string_array(batch.column(0), "aircraft_id")?;
            let timestamps = as_f64_array(batch.column(1), "timestamp")?;
            let payloads = as_binary_array(batch.column(2), "state_payload")?;

            for i in 0..batch.num_rows() {
                let id = ids.value(i).to_string();
                let ts = timestamps.value(i);
                let state = pb::AircraftState::decode(payloads.value(i))?;
                self.append_state(id, ts, state);
            }
        }

        Ok(())
    }

    fn read_events_parquet(&self, path: &Path) -> Result<(), StoreError> {
        let file_path = path.join("events.parquet");
        if !file_path.exists() {
            return Ok(());
        }

        let file = File::open(file_path)?;
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)?
            .with_batch_size(1024)
            .build()?;

        for maybe_batch in &mut reader {
            let batch = maybe_batch?;
            let ids = as_string_array(batch.column(0), "aircraft_id")?;
            let timestamps = as_f64_array(batch.column(1), "timestamp")?;
            let kinds = as_string_array(batch.column(2), "event_type")?;
            let payloads = as_binary_array(batch.column(3), "event_payload")?;

            for i in 0..batch.num_rows() {
                let id = ids.value(i).to_string();
                let ts = timestamps.value(i);
                let event = decode_event(kinds.value(i), payloads.value(i))?;
                self.append_event(id, ts, event);
            }
        }

        Ok(())
    }
}

fn as_string_array<'a>(
    array: &'a Arc<dyn Array>,
    name: &str,
) -> Result<&'a StringArray, StoreError> {
    array
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| StoreError::InvalidData(format!("{name} column type mismatch")))
}

fn as_f64_array<'a>(array: &'a Arc<dyn Array>, name: &str) -> Result<&'a Float64Array, StoreError> {
    array
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| StoreError::InvalidData(format!("{name} column type mismatch")))
}

fn as_binary_array<'a>(
    array: &'a Arc<dyn Array>,
    name: &str,
) -> Result<&'a BinaryArray, StoreError> {
    array
        .as_any()
        .downcast_ref::<BinaryArray>()
        .ok_or_else(|| StoreError::InvalidData(format!("{name} column type mismatch")))
}

fn encode_event(event: &Event) -> (String, Vec<u8>) {
    match event {
        Event::Spawn(spawn) => ("spawn".to_string(), spawn.encode_to_vec()),
        Event::Despawn(despawn) => ("despawn".to_string(), despawn.encode_to_vec()),
        Event::Custom(custom) => ("custom".to_string(), custom.as_bytes().to_vec()),
    }
}

fn decode_event(kind: &str, payload: &[u8]) -> Result<Event, StoreError> {
    match kind {
        "spawn" => Ok(Event::Spawn(Box::new(pb::AircraftSpawnInfo::decode(
            payload,
        )?))),
        "despawn" => Ok(Event::Despawn(pb::DespawnInfo::decode(payload)?)),
        "custom" => Ok(Event::Custom(String::from_utf8(payload.to_vec()).map_err(
            |e| StoreError::InvalidData(format!("invalid utf8 custom event: {e}")),
        )?)),
        _ => Err(StoreError::InvalidData(format!(
            "unknown event type: {kind}"
        ))),
    }
}

/// Return the total number of events across all aircraft.
pub fn event_count_for(store: &TimeSeriesStore) -> usize {
    store
        .data
        .iter()
        .map(|entry| entry.value().events.len())
        .sum()
}

/// Return the total number of state samples across all aircraft.
pub fn state_count_for(store: &TimeSeriesStore) -> usize {
    store
        .data
        .iter()
        .map(|entry| entry.value().states.len())
        .sum()
}

/// Return the number of distinct aircraft currently in the store.
pub fn aircraft_count_for(store: &TimeSeriesStore) -> usize {
    store.data.iter().count()
}

/// Return the global minimum and maximum state timestamps across all aircraft.
pub fn active_time_bounds(store: &TimeSeriesStore) -> Option<(f64, f64)> {
    let mut min_start: Option<f64> = None;
    let mut max_end: Option<f64> = None;

    for entry in &store.data {
        if let (Some(first), Some(last)) =
            (entry.value().states.first(), entry.value().states.last())
        {
            min_start = Some(match min_start {
                Some(current) => current.min(first.timestamp_secs),
                None => first.timestamp_secs,
            });
            max_end = Some(match max_end {
                Some(current) => current.max(last.timestamp_secs),
                None => last.timestamp_secs,
            });
        }
    }

    Some((min_start?, max_end?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mk_state(x: f64) -> pb::AircraftState {
        pb::AircraftState {
            position: Some(pb::Vector3 { x, y: 0.0, z: 0.0 }),
            velocity: Some(pb::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            attitude: Some(pb::Quaternion {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            angular_velocity: Some(pb::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            derived: None,
            control_surfaces: None,
            engines: vec![],
            custom_fields: vec![],
        }
    }

    #[test]
    fn append_and_query_range_works() {
        let store = TimeSeriesStore::new();
        store.append_state("a1".to_string(), 1.0, mk_state(1.0));
        store.append_state("a1".to_string(), 2.0, mk_state(2.0));
        store.append_state("a1".to_string(), 3.0, mk_state(3.0));

        let latest = store.get_latest(&"a1".to_string()).unwrap();
        assert_eq!(latest.timestamp_secs, 3.0);

        let range = store.get_states_range(&"a1".to_string(), 1.5, 3.0).unwrap();
        assert_eq!(range.len(), 2);
        assert_eq!(range[0].timestamp_secs, 2.0);
    }

    #[test]
    fn save_and_load_roundtrip_works() {
        let store = TimeSeriesStore::new();
        store.append_state("a1".to_string(), 1.0, mk_state(10.0));
        store.append_event("a1".to_string(), 1.5, Event::Custom("evt".to_string()));

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("fly_ruler_store_test_{nanos}"));

        store.save_to_disk(&dir).unwrap();

        let restored = TimeSeriesStore::new();
        restored.load_from_disk(&dir).unwrap();

        assert_eq!(state_count_for(&restored), 1);
        assert_eq!(event_count_for(&restored), 1);
        assert_eq!(aircraft_count_for(&restored), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn out_of_order_state_insert_keeps_sorted_order() {
        let store = TimeSeriesStore::new();
        store.append_state("a1".to_string(), 3.0, mk_state(3.0));
        store.append_state("a1".to_string(), 1.0, mk_state(1.0));
        store.append_state("a1".to_string(), 2.0, mk_state(2.0));

        let states = store
            .get_states_range(&"a1".to_string(), 0.0, 10.0)
            .unwrap();
        assert_eq!(states.len(), 3);
        assert_eq!(states[0].timestamp_secs, 1.0);
        assert_eq!(states[1].timestamp_secs, 2.0);
        assert_eq!(states[2].timestamp_secs, 3.0);
    }

    #[test]
    fn spawn_event_populates_aircraft_config() {
        let store = TimeSeriesStore::new();
        let spawn = pb::AircraftSpawnInfo {
            name: "F-16".to_string(),
            toml_config: "[aircraft]\nname='F-16'".to_string(),
            initial_state: Some(mk_state(0.0)),
        };
        store.append_event("a1".to_string(), 1.0, Event::Spawn(Box::new(spawn)));

        let entry = store.data.get("a1").unwrap();
        let config = entry.config.as_ref().unwrap();
        assert_eq!(config.name, "F-16");
        assert!(config.toml_config.contains("aircraft"));
    }

    #[test]
    fn clear_removes_all_in_memory_data() {
        let store = TimeSeriesStore::new();
        store.append_state("a1".to_string(), 1.0, mk_state(1.0));
        store.append_event("a1".to_string(), 1.2, Event::Custom("x".to_string()));
        assert_eq!(aircraft_count_for(&store), 1);

        store.clear();

        assert_eq!(aircraft_count_for(&store), 0);
        assert_eq!(state_count_for(&store), 0);
        assert_eq!(event_count_for(&store), 0);
    }

    #[test]
    fn active_time_bounds_returns_global_min_max() {
        let store = TimeSeriesStore::new();
        store.append_state("a1".to_string(), 10.0, mk_state(1.0));
        store.append_state("a1".to_string(), 20.0, mk_state(2.0));
        store.append_state("a2".to_string(), 5.0, mk_state(3.0));
        store.append_state("a2".to_string(), 25.0, mk_state(4.0));

        let bounds = active_time_bounds(&store).unwrap();
        assert_eq!(bounds.0, 5.0);
        assert_eq!(bounds.1, 25.0);
    }
}
