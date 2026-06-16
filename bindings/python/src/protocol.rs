//! Core data classes for Python bindings, aligned with protobuf schema.

use fly_ruler_proto_core::pb;
use pyo3::prelude::*;

#[pyclass(from_py_object, name = "Vector3", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyVector3 {
    pub x: f64,
    pub y: f64,
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

#[pyclass(from_py_object, name = "Quaternion", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyQuaternion {
    pub w: f64,
    pub x: f64,
    pub y: f64,
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

#[pyclass(from_py_object, name = "DerivedState", get_all, set_all)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyDerivedState {
    pub lat: f64,
    pub lon: f64,
    pub altitude: f64,
    pub alpha: f64,
    pub beta: f64,
    pub tas: f64,
    pub eas: f64,
    pub gamma: f64,
    pub chi: f64,
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
        }
    }
}

#[pymethods]
impl PyDerivedState {
    #[new]
    #[pyo3(signature = (lat, lon, altitude, alpha=0.0, beta=0.0, tas=0.0, eas=0.0, gamma=0.0, chi=0.0))]
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
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DerivedState(lat={}, lon={}, altitude={})",
            self.lat, self.lon, self.altitude
        )
    }
}

#[pyclass(from_py_object, name = "AircraftState", get_all, set_all)]
#[derive(Clone, Debug, PartialEq)]
pub struct PyAircraftState {
    pub position: PyVector3,
    pub velocity: PyVector3,
    pub attitude: PyQuaternion,
    pub angular_velocity: PyVector3,
    pub derived: Option<PyDerivedState>,
}

impl From<PyAircraftState> for pb::AircraftState {
    fn from(state: PyAircraftState) -> Self {
        Self {
            position: Some(state.position.into()),
            velocity: Some(state.velocity.into()),
            attitude: Some(state.attitude.into()),
            angular_velocity: Some(state.angular_velocity.into()),
            derived: state.derived.map(Into::into),
            custom_fields: vec![],
        }
    }
}

impl PyAircraftState {
    pub fn default_for_rust() -> Self {
        Self {
            position: PyVector3::zero(),
            velocity: PyVector3::zero(),
            attitude: PyQuaternion::identity(),
            angular_velocity: PyVector3::zero(),
            derived: None,
        }
    }
}

#[pymethods]
impl PyAircraftState {
    #[new]
    #[pyo3(signature = (position=None, velocity=None, attitude=None, angular_velocity=None, derived=None))]
    fn new(
        position: Option<PyVector3>,
        velocity: Option<PyVector3>,
        attitude: Option<PyQuaternion>,
        angular_velocity: Option<PyVector3>,
        derived: Option<PyDerivedState>,
    ) -> Self {
        Self {
            position: position.unwrap_or(PyVector3::zero()),
            velocity: velocity.unwrap_or(PyVector3::zero()),
            attitude: attitude.unwrap_or(PyQuaternion::identity()),
            angular_velocity: angular_velocity.unwrap_or(PyVector3::zero()),
            derived,
        }
    }

    #[staticmethod]
    fn hover() -> Self {
        Self::new(None, None, None, None, None)
    }

    fn __repr__(&self) -> String {
        format!(
            "AircraftState(position={:?}, velocity={:?}, attitude={:?})",
            self.position, self.velocity, self.attitude
        )
    }
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
        );

        let pb: pb::AircraftState = py.into();
        assert_eq!(pb.position.unwrap().x, 1.0);
        assert_eq!(pb.velocity.unwrap().x, 4.0);
        assert_eq!(pb.custom_fields.len(), 0);
    }
}
