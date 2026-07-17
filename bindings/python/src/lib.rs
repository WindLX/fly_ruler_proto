//! Fly Ruler Protocol Python Bindings.
//!
//! This module provides Python bindings for the Fly Ruler protocol.
//!
//! ## Usage
//! ```python
//! from fly_ruler_proto_python import (
//!     PyClient, AircraftState, Vector3, Attitude
//! )
//!
//! client = PyClient("127.0.0.1:18002")
//! ```

use pyo3::prelude::*;
use pyo3::pymodule;

mod client;
mod protocol;

mod serialize_inner {
    use fly_ruler_proto_core::PROTOCOL_VERSION;
    use pyo3::prelude::*;

    /// Return the protocol version as a string.
    #[pyfunction]
    pub fn get_protocol_version() -> String {
        PROTOCOL_VERSION.to_string()
    }
}

pub use client::*;
pub use protocol::*;

/// Python module definition.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Core state types
    m.add_class::<PyVector3>()?;
    m.add_class::<PyAttitude>()?;
    m.add_class::<PyDerivedState>()?;
    m.add_class::<PyControlSurfaceState>()?;
    m.add_class::<PyPropulsorState>()?;
    m.add_class::<PyTelemetryValueType>()?;
    m.add_class::<PyTelemetryField>()?;
    m.add_class::<PyTelemetryStreamSchema>()?;
    m.add_class::<PyAircraftState>()?;

    // Networking client/server
    m.add_class::<PyClient>()?;

    // Protocol version function
    m.add_function(wrap_pyfunction!(serialize_inner::get_protocol_version, m)?)?;

    // Protocol version (single source from core)
    m.add("PROTOCOL_VERSION", fly_ruler_proto_core::PROTOCOL_VERSION)?;

    Ok(())
}
