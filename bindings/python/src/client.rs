//! Python-facing UDP client/server bindings.
//!
//! This layer provides an aircraft-oriented client abstraction:
//! one `PyClient` instance is bound to one aircraft lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::transport::AircraftClient;
use fly_ruler_proto_core::{init_logging, LoggingConfig};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBool, PySequence};
use tokio::runtime::Runtime;
use tracing::info;

use crate::protocol::{PyAircraftState, PyTelemetryStreamSchema, PyTelemetryValueType};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn get_runtime() -> PyResult<&'static Runtime> {
    init_logging(&LoggingConfig::default());
    match RUNTIME.get() {
        Some(rt) => Ok(rt),
        None => {
            let rt = Runtime::new()
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            match RUNTIME.set(rt) {
                Ok(_) => Ok(RUNTIME.get().expect("runtime just set")),
                Err(_rt) => Ok(RUNTIME.get().expect("runtime just observed set")),
            }
        }
    }
}

fn validate_timestamp(name: &str, timestamp: Option<f64>) -> PyResult<()> {
    if timestamp.is_some_and(|value| !value.is_finite()) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "{name} must be finite"
        )));
    }
    Ok(())
}

/// Aircraft-bound client exposed to Python.
///
/// One instance corresponds to one aircraft lifecycle: connect, spawn,
/// update state, create events, despawn, and close.
#[pyclass(name = "PyClient")]
pub struct PyClient {
    addr: String,
    client_uuid: String,
    aircraft_uuid: String,
    inner: Option<AircraftClient>,
    telemetry_fields: BTreeMap<String, Vec<PyTelemetryValueType>>,
    telemetry_sequences: BTreeMap<String, u64>,
    closed: bool,
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (
        addr,
        aircraft_name,
        initial_state=None,
        toml_config="".to_string(),
        heartbeat_interval_secs=1.0,
        telemetry_schemas=None,
        spawn_timestamp=None
    ))]
    fn new(
        addr: &str,
        aircraft_name: String,
        initial_state: Option<PyAircraftState>,
        toml_config: String,
        heartbeat_interval_secs: f64,
        telemetry_schemas: Option<Vec<PyTelemetryStreamSchema>>,
        spawn_timestamp: Option<f64>,
    ) -> PyResult<Self> {
        validate_timestamp("spawn_timestamp", spawn_timestamp)?;
        if !heartbeat_interval_secs.is_finite() || heartbeat_interval_secs <= 0.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "heartbeat_interval_secs must be finite and greater than zero",
            ));
        }
        let runtime = get_runtime()?;
        let initial_state_pb: pb::AircraftState = initial_state
            .unwrap_or_else(PyAircraftState::default_for_rust)
            .into();

        let telemetry_schemas = telemetry_schemas.unwrap_or_default();
        let mut telemetry_fields = BTreeMap::new();
        for schema in &telemetry_schemas {
            if schema.stream_id.trim().is_empty()
                || telemetry_fields.contains_key(&schema.stream_id)
            {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "telemetry stream id must be non-empty and unique: {:?}",
                    schema.stream_id
                )));
            }
            if schema
                .nominal_rate_hz
                .is_some_and(|rate| !rate.is_finite() || rate <= 0.0)
            {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "telemetry stream {} has an invalid nominal rate",
                    schema.stream_id
                )));
            }
            let mut field_ids = BTreeSet::new();
            for field in &schema.fields {
                if field.field_id.trim().is_empty() || !field_ids.insert(&field.field_id) {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "telemetry field id must be non-empty and unique in stream {}: {:?}",
                        schema.stream_id, field.field_id
                    )));
                }
            }
            telemetry_fields.insert(
                schema.stream_id.clone(),
                schema.fields.iter().map(|field| field.value_type).collect(),
            );
        }
        let protobuf_schemas = telemetry_schemas.into_iter().map(Into::into).collect();

        let inner = runtime
            .block_on(async {
                AircraftClient::connect_with_telemetry_at(
                    addr,
                    &LoggingConfig::default(),
                    aircraft_name,
                    initial_state_pb,
                    toml_config,
                    heartbeat_interval_secs,
                    protobuf_schemas,
                    spawn_timestamp,
                )
                .await
            })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))?;

        let client_uuid = inner.client_uuid();
        let aircraft_uuid = inner.aircraft_uuid();

        info!(
            target: "fly_ruler_proto_python.client",
            addr = addr,
            client_uuid = client_uuid,
            aircraft_uuid = aircraft_uuid,
            "started aircraft-bound client session"
        );

        Ok(Self {
            addr: addr.to_string(),
            client_uuid,
            aircraft_uuid,
            inner: Some(inner),
            telemetry_fields,
            telemetry_sequences: BTreeMap::new(),
            closed: false,
        })
    }

    /// Return the client UUID.
    fn client_uuid(&self) -> String {
        self.client_uuid.to_string()
    }

    /// Return the aircraft UUID.
    fn aircraft_uuid(&self) -> String {
        self.aircraft_uuid.to_string()
    }

    /// Send a state update for the aircraft.
    #[pyo3(signature = (state, timestamp=None))]
    fn update_state(&mut self, state: PyAircraftState, timestamp: Option<f64>) -> PyResult<()> {
        self.ensure_open()?;
        validate_timestamp("timestamp", timestamp)?;
        self.inner
            .as_ref()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>("client is closed")
            })?
            .update_state(state.into(), timestamp)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))
    }

    /// Send a custom event for the aircraft.
    #[pyo3(signature = (event_name, timestamp=None))]
    fn create_event(&mut self, event_name: &str, timestamp: Option<f64>) -> PyResult<()> {
        self.ensure_open()?;
        validate_timestamp("timestamp", timestamp)?;
        self.inner
            .as_ref()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>("client is closed")
            })?
            .create_event(event_name.to_string(), timestamp)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))
    }

    /// Publish one telemetry frame using the schema order registered at construction.
    #[pyo3(signature = (stream_id, values, timestamp=None))]
    fn publish_telemetry(
        &mut self,
        stream_id: &str,
        values: &Bound<'_, PySequence>,
        timestamp: Option<f64>,
    ) -> PyResult<()> {
        self.ensure_open()?;
        validate_timestamp("timestamp", timestamp)?;
        let field_types = self.telemetry_fields.get(stream_id).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!("unknown telemetry stream: {stream_id}"))
        })?;
        let value_count = values.len()?;
        if value_count != field_types.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "telemetry stream {stream_id} expects {} values, got {value_count}",
                field_types.len()
            )));
        }
        let mut protobuf_values = Vec::with_capacity(field_types.len());
        for (index, value_type) in field_types.iter().enumerate() {
            let value = values.get_item(index)?;
            protobuf_values.push(telemetry_value_from_python(&value, *value_type, index)?);
        }
        let sequence = self
            .telemetry_sequences
            .entry(stream_id.to_string())
            .or_default();
        *sequence = sequence.saturating_add(1);
        let frame = pb::TelemetryFrame {
            stream_id: stream_id.to_string(),
            sequence: *sequence,
            values: protobuf_values,
        };
        self.inner
            .as_ref()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>("client is closed")
            })?
            .publish_telemetry(frame, timestamp)
            .map_err(|error| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>(error.to_string())
            })
    }

    /// Send a despawn command for the aircraft.
    #[pyo3(signature = (reason=None, timestamp=None))]
    fn despawn(&mut self, reason: Option<String>, timestamp: Option<f64>) -> PyResult<()> {
        self.ensure_open()?;
        validate_timestamp("timestamp", timestamp)?;
        self.inner
            .as_mut()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>("client is closed")
            })?
            .despawn(reason, timestamp)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))
    }

    /// Close the client, sending a best-effort despawn if needed.
    fn close(&mut self) -> PyResult<()> {
        if self.closed {
            return Ok(());
        }

        info!(
            target: "fly_ruler_proto_python.client",
            addr = self.addr,
            client_uuid = %self.client_uuid,
            aircraft_uuid = %self.aircraft_uuid,
            "closing aircraft-bound client"
        );

        let runtime = get_runtime()?;

        if let Some(inner) = self.inner.as_mut() {
            runtime
                .block_on(async { inner.close().await })
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))?;
        }
        self.inner = None;

        self.closed = true;
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "PyClient(addr='{}', aircraft_uuid='{}', client_uuid='{}', closed={})",
            self.addr, self.aircraft_uuid, self.client_uuid, self.closed
        )
    }

    fn __del__(&mut self) {
        let _ = self.close();
    }
}

fn telemetry_value_from_python(
    value: &Bound<'_, PyAny>,
    value_type: PyTelemetryValueType,
    index: usize,
) -> PyResult<pb::TelemetryValue> {
    let kind = match value_type {
        PyTelemetryValueType::F64 => {
            pb::telemetry_value::Kind::F64Value(value.extract::<f64>().map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err(format!(
                    "telemetry value {index} must be float-compatible"
                ))
            })?)
        }
        PyTelemetryValueType::I64 => {
            if value.is_instance_of::<PyBool>() {
                return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                    "telemetry value {index} must be int, not bool"
                )));
            }
            pb::telemetry_value::Kind::I64Value(value.extract::<i64>().map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err(format!(
                    "telemetry value {index} must be int"
                ))
            })?)
        }
        PyTelemetryValueType::Bool => {
            if !value.is_instance_of::<PyBool>() {
                return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                    "telemetry value {index} must be bool"
                )));
            }
            pb::telemetry_value::Kind::BoolValue(value.extract::<bool>()?)
        }
    };
    Ok(pb::TelemetryValue { kind: Some(kind) })
}

impl PyClient {
    fn ensure_open(&self) -> PyResult<()> {
        if self.closed {
            return Err(PyErr::new::<pyo3::exceptions::PyConnectionError, _>(
                "client is closed",
            ));
        }
        Ok(())
    }
}
