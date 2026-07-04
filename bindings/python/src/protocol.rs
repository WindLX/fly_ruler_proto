//! Core data classes for Python bindings, aligned with protobuf schema.

use fly_ruler_proto_core::pb;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBool, PyBytes, PyDict};

/// 3D vector with `x`, `y`, `z` components.
#[pyclass(from_py_object, name = "Vector3", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyVector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl From<PyVector3> for pb::Vector3 {
    fn from(v: PyVector3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

#[pymethods]
impl PyVector3 {
    #[new]
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Return the zero vector.
    #[staticmethod]
    fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn __repr__(&self) -> String {
        format!("Vector3(x={}, y={}, z={})", self.x, self.y, self.z)
    }
}

/// Quaternion with `w`, `x`, `y`, `z` components.
#[pyclass(from_py_object, name = "Quaternion", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyQuaternion {
    /// Real component.
    pub w: f64,
    /// Imaginary i component.
    pub x: f64,
    /// Imaginary j component.
    pub y: f64,
    /// Imaginary k component.
    pub z: f64,
}

impl From<PyQuaternion> for pb::Quaternion {
    fn from(q: PyQuaternion) -> Self {
        Self {
            w: q.w,
            x: q.x,
            y: q.y,
            z: q.z,
        }
    }
}

#[pymethods]
impl PyQuaternion {
    #[new]
    fn new(w: f64, x: f64, y: f64, z: f64) -> Self {
        Self { w, x, y, z }
    }

    /// Return the identity quaternion.
    #[staticmethod]
    fn identity() -> Self {
        Self {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Quaternion(w={}, x={}, y={}, z={})",
            self.w, self.x, self.y, self.z
        )
    }
}

/// Derived aerodynamic/navigation state.
#[pyclass(from_py_object, name = "DerivedState", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyDerivedState {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Altitude in meters.
    pub altitude: f64,
    /// Angle of attack in radians.
    pub alpha: f64,
    /// Sideslip angle in radians.
    pub beta: f64,
    /// True airspeed.
    pub tas: f64,
    /// Equivalent airspeed.
    pub eas: f64,
    /// Flight path angle in radians.
    pub gamma: f64,
    /// Track angle in radians.
    pub chi: f64,
    /// Optional indicated airspeed in meters per second.
    pub ias: Option<f64>,
    /// Optional calibrated airspeed in meters per second.
    pub cas: Option<f64>,
    /// Optional Mach number.
    pub mach: Option<f64>,
}

impl From<PyDerivedState> for pb::DerivedState {
    fn from(d: PyDerivedState) -> Self {
        Self {
            lat: d.lat,
            lon: d.lon,
            altitude: d.altitude,
            alpha: d.alpha,
            beta: d.beta,
            tas: d.tas,
            eas: d.eas,
            gamma: d.gamma,
            chi: d.chi,
            ias: d.ias,
            cas: d.cas,
            mach: d.mach,
        }
    }
}

#[pymethods]
impl PyDerivedState {
    #[new]
    #[pyo3(signature = (lat, lon, altitude, alpha=0.0, beta=0.0, tas=0.0, eas=0.0, gamma=0.0, chi=0.0, ias=None, cas=None, mach=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        lat: f64,
        lon: f64,
        altitude: f64,
        alpha: f64,
        beta: f64,
        tas: f64,
        eas: f64,
        gamma: f64,
        chi: f64,
        ias: Option<f64>,
        cas: Option<f64>,
        mach: Option<f64>,
    ) -> Self {
        Self {
            lat,
            lon,
            altitude,
            alpha,
            beta,
            tas,
            eas,
            gamma,
            chi,
            ias,
            cas,
            mach,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DerivedState(lat={}, lon={}, altitude={})",
            self.lat, self.lon, self.altitude
        )
    }
}

/// Physical control-surface state.
#[pyclass(from_py_object, name = "ControlSurfaceState", get_all, set_all)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PyControlSurfaceState {
    pub aileron_left_rad: Option<f64>,
    pub aileron_right_rad: Option<f64>,
    pub elevator_rad: Option<f64>,
    pub rudder_rad: Option<f64>,
    pub flaps_left_ratio: Option<f64>,
    pub flaps_right_ratio: Option<f64>,
    pub spoilers_ratio: Option<f64>,
}

impl From<PyControlSurfaceState> for pb::ControlSurfaceState {
    fn from(state: PyControlSurfaceState) -> Self {
        Self {
            aileron_left_rad: state.aileron_left_rad,
            aileron_right_rad: state.aileron_right_rad,
            elevator_rad: state.elevator_rad,
            rudder_rad: state.rudder_rad,
            flaps_left_ratio: state.flaps_left_ratio,
            flaps_right_ratio: state.flaps_right_ratio,
            spoilers_ratio: state.spoilers_ratio,
        }
    }
}

#[pymethods]
impl PyControlSurfaceState {
    #[new]
    #[pyo3(signature = (
        aileron_left_rad=None,
        aileron_right_rad=None,
        elevator_rad=None,
        rudder_rad=None,
        flaps_left_ratio=None,
        flaps_right_ratio=None,
        spoilers_ratio=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        aileron_left_rad: Option<f64>,
        aileron_right_rad: Option<f64>,
        elevator_rad: Option<f64>,
        rudder_rad: Option<f64>,
        flaps_left_ratio: Option<f64>,
        flaps_right_ratio: Option<f64>,
        spoilers_ratio: Option<f64>,
    ) -> Self {
        Self {
            aileron_left_rad,
            aileron_right_rad,
            elevator_rad,
            rudder_rad,
            flaps_left_ratio,
            flaps_right_ratio,
            spoilers_ratio,
        }
    }
}

/// Per-engine state.
#[pyclass(from_py_object, name = "EngineState", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyEngineState {
    pub index: u32,
    pub throttle_lever_ratio: Option<f64>,
}

impl From<PyEngineState> for pb::EngineState {
    fn from(state: PyEngineState) -> Self {
        Self {
            index: state.index,
            throttle_lever_ratio: state.throttle_lever_ratio,
        }
    }
}

#[pymethods]
impl PyEngineState {
    #[new]
    #[pyo3(signature = (index, throttle_lever_ratio=None))]
    fn new(index: u32, throttle_lever_ratio: Option<f64>) -> Self {
        Self {
            index,
            throttle_lever_ratio,
        }
    }
}

/// Full aircraft state including pose, velocity, attitude, and derived state.
#[pyclass(from_py_object, name = "AircraftState")]
#[derive(Clone, Debug, PartialEq)]
pub struct PyAircraftState {
    /// Position vector.
    #[pyo3(get, set)]
    pub position: PyVector3,
    /// Velocity vector.
    #[pyo3(get, set)]
    pub velocity: PyVector3,
    /// Attitude quaternion.
    #[pyo3(get, set)]
    pub attitude: PyQuaternion,
    /// Angular velocity vector.
    #[pyo3(get, set)]
    pub angular_velocity: PyVector3,
    /// Optional derived aerodynamic/navigation state.
    #[pyo3(get, set)]
    pub derived: Option<PyDerivedState>,
    /// Optional physical control-surface state.
    #[pyo3(get, set)]
    pub control_surfaces: Option<PyControlSurfaceState>,
    /// Per-engine states.
    #[pyo3(get, set)]
    pub engines: Vec<PyEngineState>,
    /// Extensible typed values passed through the protobuf state.
    custom_fields: Vec<pb::CustomField>,
}

impl From<PyAircraftState> for pb::AircraftState {
    fn from(state: PyAircraftState) -> Self {
        Self {
            position: Some(state.position.into()),
            velocity: Some(state.velocity.into()),
            attitude: Some(state.attitude.into()),
            angular_velocity: Some(state.angular_velocity.into()),
            derived: state.derived.map(Into::into),
            custom_fields: state.custom_fields,
            control_surfaces: state.control_surfaces.map(Into::into),
            engines: state.engines.into_iter().map(Into::into).collect(),
        }
    }
}

impl PyAircraftState {
    /// Return a default hover state for Rust-side fallback.
    pub fn default_for_rust() -> Self {
        Self {
            position: PyVector3::zero(),
            velocity: PyVector3::zero(),
            attitude: PyQuaternion::identity(),
            angular_velocity: PyVector3::zero(),
            derived: None,
            control_surfaces: None,
            engines: vec![],
            custom_fields: vec![],
        }
    }
}

#[pymethods]
impl PyAircraftState {
    #[new]
    #[pyo3(signature = (
        position=None,
        velocity=None,
        attitude=None,
        angular_velocity=None,
        derived=None,
        control_surfaces=None,
        engines=None,
        custom_fields=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        position: Option<PyVector3>,
        velocity: Option<PyVector3>,
        attitude: Option<PyQuaternion>,
        angular_velocity: Option<PyVector3>,
        derived: Option<PyDerivedState>,
        control_surfaces: Option<PyControlSurfaceState>,
        engines: Option<Vec<PyEngineState>>,
        custom_fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        Ok(Self {
            position: position.unwrap_or(PyVector3::zero()),
            velocity: velocity.unwrap_or(PyVector3::zero()),
            attitude: attitude.unwrap_or(PyQuaternion::identity()),
            angular_velocity: angular_velocity.unwrap_or(PyVector3::zero()),
            derived,
            control_surfaces,
            engines: engines.unwrap_or_default(),
            custom_fields: custom_fields
                .map(custom_fields_from_dict)
                .transpose()?
                .unwrap_or_default(),
        })
    }

    /// Return a default hover state.
    #[staticmethod]
    fn hover() -> Self {
        Self::default_for_rust()
    }

    /// Return custom fields as a regular Python dictionary.
    #[getter]
    fn custom_fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let result = PyDict::new(py);
        for field in &self.custom_fields {
            let Some(value) = field.value.as_ref().and_then(|value| value.kind.as_ref()) else {
                continue;
            };
            match value {
                pb::field_value::Kind::F64Value(value) => {
                    result.set_item(&field.field_id, value)?;
                }
                pb::field_value::Kind::I64Value(value) => {
                    result.set_item(&field.field_id, value)?;
                }
                pb::field_value::Kind::BoolValue(value) => {
                    result.set_item(&field.field_id, value)?;
                }
                pb::field_value::Kind::StringValue(value) => {
                    result.set_item(&field.field_id, value)?;
                }
                pb::field_value::Kind::BytesValue(value) => {
                    result.set_item(&field.field_id, PyBytes::new(py, value))?;
                }
            }
        }
        Ok(result)
    }

    /// Replace all custom fields from a regular Python dictionary.
    #[setter]
    fn set_custom_fields(&mut self, values: &Bound<'_, PyDict>) -> PyResult<()> {
        self.custom_fields = custom_fields_from_dict(values)?;
        Ok(())
    }

    /// Add or replace a single custom field.
    fn set_custom_field(&mut self, field_id: String, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let field = custom_field_from_value(field_id.clone(), value)?;
        self.custom_fields
            .retain(|existing| existing.field_id != field_id);
        self.custom_fields.push(field);
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "AircraftState(position={:?}, velocity={:?}, attitude={:?})",
            self.position, self.velocity, self.attitude
        )
    }
}

fn custom_fields_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Vec<pb::CustomField>> {
    values
        .iter()
        .map(|(key, value)| custom_field_from_value(key.extract()?, &value))
        .collect()
}

fn custom_field_from_value(
    field_id: String,
    value: &Bound<'_, PyAny>,
) -> PyResult<pb::CustomField> {
    let kind = if value.is_instance_of::<PyBool>() {
        pb::field_value::Kind::BoolValue(value.extract()?)
    } else if let Ok(value) = value.extract::<i64>() {
        pb::field_value::Kind::I64Value(value)
    } else if let Ok(value) = value.extract::<f64>() {
        pb::field_value::Kind::F64Value(value)
    } else if let Ok(value) = value.extract::<String>() {
        pb::field_value::Kind::StringValue(value)
    } else if let Ok(value) = value.cast::<PyBytes>() {
        pb::field_value::Kind::BytesValue(value.as_bytes().to_vec())
    } else {
        return Err(pyo3::exceptions::PyTypeError::new_err(format!(
            "custom field {field_id:?} must be float, int, bool, str, or bytes"
        )));
    };

    Ok(pb::CustomField {
        field_id,
        value: Some(pb::FieldValue { kind: Some(kind) }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aircraft_state_converts_to_proto() {
        let py = PyAircraftState::new(
            Some(PyVector3::new(1.0, 2.0, 3.0)),
            Some(PyVector3::new(4.0, 5.0, 6.0)),
            Some(PyQuaternion::new(1.0, 0.0, 0.0, 0.0)),
            Some(PyVector3::new(0.1, 0.2, 0.3)),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let pb: pb::AircraftState = py.into();
        assert_eq!(pb.position.unwrap().x, 1.0);
        assert_eq!(pb.velocity.unwrap().x, 4.0);
        assert_eq!(pb.custom_fields.len(), 0);
    }
}
