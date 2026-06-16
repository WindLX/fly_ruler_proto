//! Python-facing UDP client/server bindings.
//!
//! This layer provides an aircraft-oriented client abstraction:
//! one `PyClient` instance is bound to one aircraft lifecycle.

use std::sync::OnceLock;

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::transport::{AircraftClient, Server as RustServer};
use pyo3::prelude::*;
use tokio::runtime::Runtime;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

use crate::protocol::PyAircraftState;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static LOGGING_INIT: OnceLock<()> = OnceLock::new();

const DEFAULT_LOG_FILTER: &str =
    "warn,fly_ruler_proto_python.client=info,fly_ruler_proto_python.server=info,fly_ruler_proto_core.runtime=warn,fly_ruler_proto_core.store=warn,fly_ruler_proto_core.transport=warn";

fn init_logging() {
    let _ = LOGGING_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_names(true)
            .try_init();
    });
}

fn get_runtime() -> &'static Runtime {
    init_logging();
    RUNTIME.get_or_init(|| Runtime::new().expect("failed to create tokio runtime"))
}

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
        let runtime = get_runtime();
        let initial_state_pb: pb::AircraftState = initial_state
            .unwrap_or_else(PyAircraftState::default_for_rust)
            .into();

        let inner = runtime
            .block_on(async {
                AircraftClient::connect(
                    addr,
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

    fn client_uuid(&self) -> String {
        self.client_uuid.to_string()
    }

    fn aircraft_uuid(&self) -> String {
        self.aircraft_uuid.to_string()
    }

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

        let runtime = get_runtime();

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

#[pyclass(name = "PyServer")]
pub struct PyServer {
    inner: Option<RustServer>,
    addr: String,
}

#[pymethods]
impl PyServer {
    #[new]
    fn new(addr: &str) -> PyResult<Self> {
        let runtime = get_runtime();
        info!(target: "fly_ruler_proto_python.server", addr = addr, "binding UDP server");
        let server = runtime
            .block_on(async { RustServer::bind(addr).await })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))?;

        Ok(Self {
            inner: Some(server),
            addr: addr.to_string(),
        })
    }

    fn local_addr(&self) -> PyResult<String> {
        let addr = self
            .inner
            .as_ref()
            .map(|s| {
                s.local_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| self.addr.clone())
            })
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyConnectionError, _>("not listening"))?;

        debug!(target: "fly_ruler_proto_python.server", local_addr = addr, "queried local_addr");
        Ok(addr)
    }

    fn close(&mut self) {
        info!(target: "fly_ruler_proto_python.server", "closing server handle");
        self.inner = None;
    }

    fn __repr__(&self) -> String {
        format!("PyServer(addr='{}')", self.addr)
    }
}
