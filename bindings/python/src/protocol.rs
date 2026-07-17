//! Core data classes for Python bindings, aligned with protobuf schema.

use fly_ruler_proto_core::{pb, Attitude};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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

/// Validated BODY-FRD to local-NED attitude.
///
/// Quaternions use Hamilton scalar-first `[w, x, y, z]`, rotation matrices
/// are 3x3 row-major, and Euler angles are Z-Y-X `[roll, pitch, yaw]` in rad.
#[pyclass(from_py_object, name = "Attitude", frozen)]
#[derive(Clone, Debug, PartialEq)]
pub struct PyAttitude(pub Attitude);

impl From<PyAttitude> for pb::Quaternion {
    fn from(attitude: PyAttitude) -> Self {
        Self::from(&attitude.0)
    }
}

#[pymethods]
impl PyAttitude {
    /// Return the identity attitude.
    #[staticmethod]
    fn identity() -> Self {
        Self(Attitude::identity())
    }

    /// Construct from Hamilton scalar-first `[w, x, y, z]` values.
    #[staticmethod]
    fn from_quaternion(values: Vec<f64>) -> PyResult<Self> {
        let values: [f64; 4] = values
            .try_into()
            .map_err(|_| PyValueError::new_err("quaternion must contain exactly 4 values"))?;
        Attitude::from_quaternion(values)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Construct from a row-major 3x3 BODY-FRD to local-NED rotation matrix.
    #[staticmethod]
    fn from_rotation_matrix(values: Vec<f64>) -> PyResult<Self> {
        let values: [f64; 9] = values
            .try_into()
            .map_err(|_| PyValueError::new_err("rotation matrix must contain exactly 9 values"))?;
        Attitude::from_rotation_matrix(values)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Construct from Z-Y-X `[roll, pitch, yaw]` Euler angles in radians.
    #[staticmethod]
    fn from_euler(values: Vec<f64>) -> PyResult<Self> {
        let values: [f64; 3] = values
            .try_into()
            .map_err(|_| PyValueError::new_err("Euler angles must contain exactly 3 values"))?;
        Attitude::from_euler(values)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Return Hamilton scalar-first quaternion values.
    #[getter]
    fn quaternion(&self) -> (f64, f64, f64, f64) {
        self.0.quaternion().into()
    }

    /// Return the row-major BODY-FRD to local-NED rotation matrix.
    #[getter]
    fn rotation_matrix(&self) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        self.0.rotation_matrix().into()
    }

    /// Return Z-Y-X `[roll, pitch, yaw]` Euler angles in radians.
    #[getter]
    fn euler(&self) -> (f64, f64, f64) {
        self.0.euler().into()
    }

    fn __repr__(&self) -> String {
        format!("Attitude(quaternion={:?})", self.0.quaternion())
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
    /// Optional ground speed in meters per second.
    pub ground_speed: Option<f64>,
    /// Optional vertical speed, positive upward, in meters per second.
    pub vertical_speed: Option<f64>,
    /// Optional dynamic pressure in pascals.
    pub dynamic_pressure: Option<f64>,
    /// Optional normal load factor.
    pub normal_load_factor: Option<f64>,
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
            ground_speed: d.ground_speed,
            vertical_speed: d.vertical_speed,
            dynamic_pressure: d.dynamic_pressure,
            normal_load_factor: d.normal_load_factor,
        }
    }
}

#[pymethods]
impl PyDerivedState {
    #[new]
    #[pyo3(signature = (lat, lon, altitude, alpha=0.0, beta=0.0, tas=0.0, eas=0.0, gamma=0.0, chi=0.0, ias=None, cas=None, mach=None, ground_speed=None, vertical_speed=None, dynamic_pressure=None, normal_load_factor=None))]
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
        ground_speed: Option<f64>,
        vertical_speed: Option<f64>,
        dynamic_pressure: Option<f64>,
        normal_load_factor: Option<f64>,
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
            ground_speed,
            vertical_speed,
            dynamic_pressure,
            normal_load_factor,
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

/// Cross-aircraft propulsor state for jets, propellers, and rotors.
#[pyclass(from_py_object, name = "PropulsorState", get_all, set_all)]
#[derive(Clone, Debug, PartialEq)]
pub struct PyPropulsorState {
    pub propulsor_id: String,
    pub kind: i32,
    pub throttle_ratio: Option<f64>,
    pub rpm: Option<f64>,
    pub blade_pitch_rad: Option<f64>,
    pub thrust_newton: Option<f64>,
    pub torque_newton_meter: Option<f64>,
    pub index: Option<u32>,
}

/// Scalar type declared by one telemetry field.
#[pyclass(eq, eq_int, from_py_object, name = "TelemetryValueType")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PyTelemetryValueType {
    F64 = 1,
    I64 = 2,
    Bool = 3,
}

impl From<PyTelemetryValueType> for pb::TelemetryValueType {
    fn from(value: PyTelemetryValueType) -> Self {
        match value {
            PyTelemetryValueType::F64 => Self::F64,
            PyTelemetryValueType::I64 => Self::I64,
            PyTelemetryValueType::Bool => Self::Bool,
        }
    }
}

/// Metadata for one scalar telemetry field.
#[pyclass(from_py_object, name = "TelemetryField", get_all)]
#[derive(Clone, Debug, PartialEq)]
pub struct PyTelemetryField {
    pub field_id: String,
    pub label: String,
    pub group: String,
    pub unit: String,
    pub description: String,
    pub value_type: PyTelemetryValueType,
}

impl From<PyTelemetryField> for pb::TelemetryField {
    fn from(field: PyTelemetryField) -> Self {
        Self {
            field_id: field.field_id,
            label: field.label,
            group: field.group,
            unit: field.unit,
            description: field.description,
            value_type: pb::TelemetryValueType::from(field.value_type) as i32,
        }
    }
}

#[pymethods]
impl PyTelemetryField {
    #[new]
    #[pyo3(signature = (field_id, label="".to_string(), group="".to_string(), unit="".to_string(), description="".to_string(), value_type=PyTelemetryValueType::F64))]
    fn new(
        field_id: String,
        label: String,
        group: String,
        unit: String,
        description: String,
        value_type: PyTelemetryValueType,
    ) -> Self {
        Self {
            field_id,
            label,
            group,
            unit,
            description,
            value_type,
        }
    }
}

/// Immutable schema for one independently sampled telemetry stream.
#[pyclass(from_py_object, name = "TelemetryStreamSchema", get_all)]
#[derive(Clone, Debug, PartialEq)]
pub struct PyTelemetryStreamSchema {
    pub stream_id: String,
    pub name: String,
    pub nominal_rate_hz: Option<f64>,
    pub fields: Vec<PyTelemetryField>,
}

impl From<PyTelemetryStreamSchema> for pb::TelemetryStreamSchema {
    fn from(schema: PyTelemetryStreamSchema) -> Self {
        Self {
            stream_id: schema.stream_id,
            name: schema.name,
            nominal_rate_hz: schema.nominal_rate_hz,
            fields: schema.fields.into_iter().map(Into::into).collect(),
        }
    }
}

#[pymethods]
impl PyTelemetryStreamSchema {
    #[new]
    #[pyo3(signature = (stream_id, fields, name="".to_string(), nominal_rate_hz=None))]
    fn new(
        stream_id: String,
        fields: Vec<PyTelemetryField>,
        name: String,
        nominal_rate_hz: Option<f64>,
    ) -> Self {
        Self {
            stream_id,
            name,
            nominal_rate_hz,
            fields,
        }
    }
}

impl From<PyPropulsorState> for pb::PropulsorState {
    fn from(state: PyPropulsorState) -> Self {
        Self {
            propulsor_id: state.propulsor_id,
            kind: state.kind,
            throttle_ratio: state.throttle_ratio,
            rpm: state.rpm,
            blade_pitch_rad: state.blade_pitch_rad,
            thrust_newton: state.thrust_newton,
            torque_newton_meter: state.torque_newton_meter,
            index: state.index,
        }
    }
}

#[pymethods]
impl PyPropulsorState {
    #[new]
    #[pyo3(signature = (propulsor_id, kind=0, throttle_ratio=None, rpm=None, blade_pitch_rad=None, thrust_newton=None, torque_newton_meter=None, index=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        propulsor_id: String,
        kind: i32,
        throttle_ratio: Option<f64>,
        rpm: Option<f64>,
        blade_pitch_rad: Option<f64>,
        thrust_newton: Option<f64>,
        torque_newton_meter: Option<f64>,
        index: Option<u32>,
    ) -> Self {
        Self {
            propulsor_id,
            kind,
            throttle_ratio,
            rpm,
            blade_pitch_rad,
            thrust_newton,
            torque_newton_meter,
            index,
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
    /// Validated attitude.
    #[pyo3(get, set)]
    pub attitude: PyAttitude,
    /// Angular velocity vector.
    #[pyo3(get, set)]
    pub angular_velocity: PyVector3,
    /// Optional derived aerodynamic/navigation state.
    #[pyo3(get, set)]
    pub derived: Option<PyDerivedState>,
    /// Optional physical control-surface state.
    #[pyo3(get, set)]
    pub control_surfaces: Option<PyControlSurfaceState>,
    /// Optional Body-FRD linear acceleration.
    #[pyo3(get, set)]
    pub linear_acceleration_body: Option<PyVector3>,
    /// Cross-aircraft propulsor states.
    #[pyo3(get, set)]
    pub propulsors: Vec<PyPropulsorState>,
}

impl From<PyAircraftState> for pb::AircraftState {
    fn from(state: PyAircraftState) -> Self {
        Self {
            position: Some(state.position.into()),
            velocity: Some(state.velocity.into()),
            attitude: Some(state.attitude.into()),
            angular_velocity: Some(state.angular_velocity.into()),
            derived: state.derived.map(Into::into),
            control_surfaces: state.control_surfaces.map(Into::into),
            linear_acceleration_body: state.linear_acceleration_body.map(Into::into),
            propulsors: state.propulsors.into_iter().map(Into::into).collect(),
        }
    }
}

impl PyAircraftState {
    /// Return a default hover state for Rust-side fallback.
    pub fn default_for_rust() -> Self {
        Self {
            position: PyVector3::zero(),
            velocity: PyVector3::zero(),
            attitude: PyAttitude::identity(),
            angular_velocity: PyVector3::zero(),
            derived: None,
            control_surfaces: None,
            linear_acceleration_body: None,
            propulsors: vec![],
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
        linear_acceleration_body=None,
        propulsors=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        position: Option<PyVector3>,
        velocity: Option<PyVector3>,
        attitude: Option<PyAttitude>,
        angular_velocity: Option<PyVector3>,
        derived: Option<PyDerivedState>,
        control_surfaces: Option<PyControlSurfaceState>,
        linear_acceleration_body: Option<PyVector3>,
        propulsors: Option<Vec<PyPropulsorState>>,
    ) -> PyResult<Self> {
        Ok(Self {
            position: position.unwrap_or(PyVector3::zero()),
            velocity: velocity.unwrap_or(PyVector3::zero()),
            attitude: attitude.unwrap_or(PyAttitude::identity()),
            angular_velocity: angular_velocity.unwrap_or(PyVector3::zero()),
            derived,
            control_surfaces,
            linear_acceleration_body,
            propulsors: propulsors.unwrap_or_default(),
        })
    }

    /// Return a default hover state.
    #[staticmethod]
    fn hover() -> Self {
        Self::default_for_rust()
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
            Some(PyAttitude::identity()),
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
        assert!(pb.propulsors.is_empty());
    }
}
