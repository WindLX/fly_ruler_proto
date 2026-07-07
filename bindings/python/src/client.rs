//! Python-facing UDP client/server bindings.
//!
//! This layer provides an aircraft-oriented client abstraction:
//! one `PyClient` instance is bound to one aircraft lifecycle.

use std::sync::OnceLock;

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::transport::AircraftClient;
use fly_ruler_proto_core::{init_logging, LoggingConfig};
use pyo3::prelude::*;
use tokio::runtime::Runtime;
use tracing::info;

use crate::protocol::PyAircraftState;

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
        heartbeat_interval_secs=1.0
    ))]
    fn new(
        addr: &str,
        aircraft_name: String,
        initial_state: Option<PyAircraftState>,
        toml_config: String,
        heartbeat_interval_secs: f64,
    ) -> PyResult<Self> {
        let runtime = get_runtime()?;
        let initial_state_pb: pb::AircraftState = initial_state
            .unwrap_or_else(PyAircraftState::default_for_rust)
            .into();

        let inner = runtime
            .block_on(async {
                AircraftClient::connect(
                    addr,
                    &LoggingConfig::default(),
                    aircraft_name,
                    initial_state_pb,
                    toml_config,
                    heartbeat_interval_secs,
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
        self.inner
            .as_ref()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyConnectionError, _>("client is closed")
            })?
            .create_event(event_name.to_string(), timestamp)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))
    }

    /// Send a despawn command for the aircraft.
    #[pyo3(signature = (reason=None, timestamp=None))]
    fn despawn(&mut self, reason: Option<String>, timestamp: Option<f64>) -> PyResult<()> {
        self.ensure_open()?;
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
