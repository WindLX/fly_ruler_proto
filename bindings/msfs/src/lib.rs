//! FlyRuler to Microsoft Flight Simulator 2024 bridge logic.

use std::f64::consts::TAU;

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::{Event, TimeSeriesStore};
use thiserror::Error;

/// Reserved FlyRuler custom field identifiers understood by the MSFS bridge.
pub mod fields {
    /// Left aileron deflection in radians.
    pub const AILERON_LEFT: &str = "flyruler.control.aileron_left_rad";
    /// Right aileron deflection in radians.
    pub const AILERON_RIGHT: &str = "flyruler.control.aileron_right_rad";
    /// Elevator deflection in radians.
    pub const ELEVATOR: &str = "flyruler.control.elevator_rad";
    /// Rudder deflection in radians.
    pub const RUDDER: &str = "flyruler.control.rudder_rad";
    /// Left trailing-edge flap ratio in the inclusive range 0..=1.
    pub const FLAPS_LEFT: &str = "flyruler.control.flaps_left_ratio";
    /// Right trailing-edge flap ratio in the inclusive range 0..=1.
    pub const FLAPS_RIGHT: &str = "flyruler.control.flaps_right_ratio";
    /// Symmetric spoiler handle ratio in the inclusive range 0..=1.
    pub const SPOILERS: &str = "flyruler.control.spoilers_ratio";
}

/// A pose expressed using writable MSFS simulation variables.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct MsfsPose {
    /// WGS-84 latitude in degrees.
    pub latitude_deg: f64,
    /// WGS-84 longitude in degrees.
    pub longitude_deg: f64,
    /// Mean-sea-level altitude in meters.
    pub altitude_m: f64,
    /// MSFS pitch angle in radians (positive nose-down).
    pub pitch_rad: f64,
    /// MSFS bank angle in radians (positive left-wing-down).
    pub bank_rad: f64,
    /// True heading in radians, normalized to 0..2π.
    pub heading_true_rad: f64,
}

/// Optional physical control-surface values.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ControlSurfaces {
    /// Left aileron deflection in radians.
    pub aileron_left_rad: Option<f64>,
    /// Right aileron deflection in radians.
    pub aileron_right_rad: Option<f64>,
    /// Elevator deflection in radians.
    pub elevator_rad: Option<f64>,
    /// Rudder deflection in radians.
    pub rudder_rad: Option<f64>,
    /// Left flap deployment ratio.
    pub flaps_left_ratio: Option<f64>,
    /// Right flap deployment ratio.
    pub flaps_right_ratio: Option<f64>,
    /// Symmetric spoiler deployment ratio.
    pub spoilers_ratio: Option<f64>,
}

/// Air-relative velocity and angular rates expressed in MSFS body axes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct MsfsAirData {
    /// True airspeed in meters per second.
    pub true_airspeed_mps: f64,
    /// Lateral velocity, positive right.
    pub velocity_body_x_mps: f64,
    /// Vertical velocity, positive up.
    pub velocity_body_y_mps: f64,
    /// Longitudinal velocity, positive forward.
    pub velocity_body_z_mps: f64,
    /// Pitch rate about the lateral axis.
    pub rotation_body_x_radps: f64,
    /// Yaw rate about the upward vertical axis.
    pub rotation_body_y_radps: f64,
    /// Roll rate about the forward longitudinal axis.
    pub rotation_body_z_radps: f64,
}

/// Optional per-engine throttle lever positions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngineThrottles {
    /// Engine indices 1 through 4 stored at array indices 0 through 3.
    pub ratios: [Option<f64>; 4],
}

/// A validated frame ready to write to MSFS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MsfsFrame {
    /// Geodetic pose.
    pub pose: MsfsPose,
    /// Optional control surfaces.
    pub controls: ControlSurfaces,
    /// Optional airdata; invalid optional airdata does not invalidate the pose.
    pub airdata: Option<MsfsAirData>,
    /// Optional per-engine throttle values.
    pub engines: EngineThrottles,
}

/// Input validation failures.
#[derive(Debug, Error, PartialEq)]
pub enum FrameError {
    /// The state does not contain geodetic navigation data.
    #[error("aircraft state is missing derived lat/lon/altitude")]
    MissingDerived,
    /// The state does not contain an attitude quaternion.
    #[error("aircraft state is missing attitude quaternion")]
    MissingAttitude,
    /// A value is not finite.
    #[error("{0} must be finite")]
    NonFinite(&'static str),
    /// Latitude is outside its valid range.
    #[error("latitude must be in -90..=90 degrees")]
    LatitudeRange,
    /// Longitude is outside its valid range.
    #[error("longitude must be in -180..=180 degrees")]
    LongitudeRange,
    /// Quaternion magnitude is too close to zero.
    #[error("attitude quaternion has zero magnitude")]
    ZeroQuaternion,
}

/// Surface identifiers used by the simulator abstraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Left aileron.
    AileronLeft,
    /// Right aileron.
    AileronRight,
    /// Elevator.
    Elevator,
    /// Rudder.
    Rudder,
    /// Left trailing-edge flap.
    FlapsLeft,
    /// Right trailing-edge flap.
    FlapsRight,
    /// Symmetric spoiler handle.
    Spoilers,
}

/// Minimal simulator operations needed by the bridge state machine.
pub trait Simulator {
    /// Error produced by the simulator implementation.
    type Error;

    /// Freeze or release MSFS position and attitude integration.
    fn set_frozen(&mut self, frozen: bool) -> Result<(), Self::Error>;
    /// Write a geodetic pose.
    fn set_pose(&mut self, pose: MsfsPose) -> Result<(), Self::Error>;
    /// Write one optional control-surface value.
    fn set_surface(&mut self, surface: Surface, value: f64) -> Result<(), Self::Error>;
    /// Write body velocity, angular rates, and true airspeed.
    fn set_airdata(&mut self, airdata: MsfsAirData) -> Result<(), Self::Error>;
    /// Write one indexed engine throttle lever position.
    fn set_engine_throttle(&mut self, index: u32, ratio: f64) -> Result<(), Self::Error>;
}

/// Return whether an aircraft's latest lifecycle event leaves it spawned.
pub fn is_spawned(store: &TimeSeriesStore, id: &str) -> bool {
    let id = id.to_owned();
    store
        .get_events_range(&id, f64::NEG_INFINITY, f64::INFINITY)
        .and_then(|events| {
            events
                .into_iter()
                .rev()
                .find_map(|entry| match entry.event {
                    Event::Spawn(_) => Some(true),
                    Event::Despawn(_) => Some(false),
                    Event::Custom(_) => None,
                })
        })
        .unwrap_or(false)
}

/// Select a requested active aircraft or the earliest active spawn.
pub fn select_aircraft(store: &TimeSeriesStore, requested: Option<&str>) -> Option<String> {
    if let Some(id) = requested {
        return is_spawned(store, id).then(|| id.to_owned());
    }

    store
        .get_aircraft_ids()
        .into_iter()
        .filter(|id| is_spawned(store, id))
        .filter_map(|id| first_spawn_timestamp(store, &id).map(|timestamp| (timestamp, id)))
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        })
        .map(|(_, id)| id)
}

fn first_spawn_timestamp(store: &TimeSeriesStore, id: &String) -> Option<f64> {
    store
        .get_events_range(id, f64::NEG_INFINITY, f64::INFINITY)?
        .into_iter()
        .find_map(|entry| matches!(entry.event, Event::Spawn(_)).then_some(entry.timestamp_secs))
}

/// Owns freeze lifecycle and applies validated frames to a simulator.
pub struct BridgeSession<S> {
    simulator: S,
    frozen: bool,
}

impl<S: Simulator> BridgeSession<S> {
    /// Create an idle session.
    pub fn new(simulator: S) -> Self {
        Self {
            simulator,
            frozen: false,
        }
    }

    /// Apply one frame, freezing the simulator immediately before the first write.
    pub fn apply(&mut self, frame: MsfsFrame) -> Result<(), S::Error> {
        if !self.frozen {
            self.simulator.set_frozen(true)?;
            self.frozen = true;
        }
        self.simulator.set_pose(frame.pose)?;
        if let Some(airdata) = frame.airdata {
            self.simulator.set_airdata(airdata)?;
        }
        for (surface, value) in control_values(frame.controls) {
            self.simulator.set_surface(surface, value)?;
        }
        for (offset, ratio) in frame.engines.ratios.into_iter().enumerate() {
            if let Some(ratio) = ratio {
                self.simulator
                    .set_engine_throttle(offset as u32 + 1, ratio)?;
            }
        }
        Ok(())
    }

    /// Release simulator motion if this session acquired it.
    pub fn release(&mut self) -> Result<(), S::Error> {
        if self.frozen {
            self.simulator.set_frozen(false)?;
            self.frozen = false;
        }
        Ok(())
    }

    /// Return whether the session currently holds the simulator frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Return a mutable reference to the underlying simulator.
    pub fn simulator_mut(&mut self) -> &mut S {
        &mut self.simulator
    }
}

/// Convert a FlyRuler state into the MSFS visual contract.
pub fn frame_from_state(state: &pb::AircraftState) -> Result<MsfsFrame, FrameError> {
    let derived = state.derived.as_ref().ok_or(FrameError::MissingDerived)?;
    let attitude = state.attitude.as_ref().ok_or(FrameError::MissingAttitude)?;

    finite(derived.lat, "latitude")?;
    finite(derived.lon, "longitude")?;
    finite(derived.altitude, "altitude")?;
    if !(-90.0..=90.0).contains(&derived.lat) {
        return Err(FrameError::LatitudeRange);
    }
    if !(-180.0..=180.0).contains(&derived.lon) {
        return Err(FrameError::LongitudeRange);
    }

    for (value, name) in [
        (attitude.w, "quaternion w"),
        (attitude.x, "quaternion x"),
        (attitude.y, "quaternion y"),
        (attitude.z, "quaternion z"),
    ] {
        finite(value, name)?;
    }

    let norm = (attitude.w * attitude.w
        + attitude.x * attitude.x
        + attitude.y * attitude.y
        + attitude.z * attitude.z)
        .sqrt();
    if norm <= f64::EPSILON {
        return Err(FrameError::ZeroQuaternion);
    }
    let (w, x, y, z) = (
        attitude.w / norm,
        attitude.x / norm,
        attitude.y / norm,
        attitude.z / norm,
    );

    // Hamilton quaternion rotating body-FRD into local NED.
    let roll = (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
    let sin_pitch = (2.0 * (w * y - z * x)).clamp(-1.0, 1.0);
    let pitch = sin_pitch.asin();
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));

    Ok(MsfsFrame {
        pose: MsfsPose {
            latitude_deg: derived.lat,
            longitude_deg: derived.lon,
            altitude_m: derived.altitude,
            pitch_rad: -pitch,
            bank_rad: -roll,
            heading_true_rad: yaw.rem_euclid(TAU),
        },
        controls: controls_from_state(state),
        airdata: airdata_from_state(state),
        engines: engines_from_state(state),
    })
}

/// Describe invalid optional fields isolated from an otherwise valid frame.
pub fn optional_field_warnings(state: &pb::AircraftState) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    if state.derived.is_some() && airdata_from_state(state).is_none() {
        warnings.push("invalid alpha, beta, or true airspeed; airdata was not written");
    }

    if let Some(controls) = state.control_surfaces.as_ref() {
        for (value, name) in [
            (controls.aileron_left_rad, "invalid left aileron angle"),
            (controls.aileron_right_rad, "invalid right aileron angle"),
            (controls.elevator_rad, "invalid elevator angle"),
            (controls.rudder_rad, "invalid rudder angle"),
        ] {
            if value.is_some_and(|value| !value.is_finite()) {
                warnings.push(name);
            }
        }
        for (value, name) in [
            (controls.flaps_left_ratio, "invalid left flap ratio"),
            (controls.flaps_right_ratio, "invalid right flap ratio"),
            (controls.spoilers_ratio, "invalid spoiler ratio"),
        ] {
            if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
                warnings.push(name);
            }
        }
    }

    for engine in &state.engines {
        if !(1..=4).contains(&engine.index) {
            warnings.push("invalid engine index; expected 1 through 4");
        } else if engine
            .throttle_lever_ratio
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            warnings.push("invalid engine throttle ratio");
        }
    }
    warnings
}

fn finite(value: f64, name: &'static str) -> Result<(), FrameError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(FrameError::NonFinite(name))
    }
}

fn field_f64(field: &pb::CustomField) -> Option<f64> {
    match field.value.as_ref()?.kind.as_ref()? {
        pb::field_value::Kind::F64Value(value) if value.is_finite() => Some(*value),
        pb::field_value::Kind::I64Value(value) => Some(*value as f64),
        _ => None,
    }
}

fn controls_from_fields(fields_in: &[pb::CustomField]) -> ControlSurfaces {
    let mut out = ControlSurfaces::default();
    for field in fields_in {
        let Some(value) = field_f64(field) else {
            continue;
        };
        match field.field_id.as_str() {
            fields::AILERON_LEFT => out.aileron_left_rad = Some(value),
            fields::AILERON_RIGHT => out.aileron_right_rad = Some(value),
            fields::ELEVATOR => out.elevator_rad = Some(value),
            fields::RUDDER => out.rudder_rad = Some(value),
            fields::FLAPS_LEFT if (0.0..=1.0).contains(&value) => {
                out.flaps_left_ratio = Some(value);
            }
            fields::FLAPS_RIGHT if (0.0..=1.0).contains(&value) => {
                out.flaps_right_ratio = Some(value);
            }
            fields::SPOILERS if (0.0..=1.0).contains(&value) => {
                out.spoilers_ratio = Some(value);
            }
            _ => {}
        }
    }
    out
}

fn valid_finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn valid_ratio(value: Option<f64>) -> Option<f64> {
    valid_finite(value).filter(|value| (0.0..=1.0).contains(value))
}

fn controls_from_state(state: &pb::AircraftState) -> ControlSurfaces {
    let mut out = controls_from_fields(&state.custom_fields);
    let Some(standard) = state.control_surfaces.as_ref() else {
        return out;
    };

    if standard.aileron_left_rad.is_some() {
        out.aileron_left_rad = valid_finite(standard.aileron_left_rad);
    }
    if standard.aileron_right_rad.is_some() {
        out.aileron_right_rad = valid_finite(standard.aileron_right_rad);
    }
    if standard.elevator_rad.is_some() {
        out.elevator_rad = valid_finite(standard.elevator_rad);
    }
    if standard.rudder_rad.is_some() {
        out.rudder_rad = valid_finite(standard.rudder_rad);
    }
    if standard.flaps_left_ratio.is_some() {
        out.flaps_left_ratio = valid_ratio(standard.flaps_left_ratio);
    }
    if standard.flaps_right_ratio.is_some() {
        out.flaps_right_ratio = valid_ratio(standard.flaps_right_ratio);
    }
    if standard.spoilers_ratio.is_some() {
        out.spoilers_ratio = valid_ratio(standard.spoilers_ratio);
    }
    out
}

fn airdata_from_state(state: &pb::AircraftState) -> Option<MsfsAirData> {
    let derived = state.derived.as_ref()?;
    let (tas, alpha, beta) = (derived.tas, derived.alpha, derived.beta);
    if !tas.is_finite()
        || tas < 0.0
        || !alpha.is_finite()
        || !beta.is_finite()
        || beta.abs() > std::f64::consts::FRAC_PI_2
    {
        return None;
    }

    // Reconstruct body-FRD air-relative velocity.
    let u_forward = tas * alpha.cos() * beta.cos();
    let v_right = tas * beta.sin();
    let w_down = tas * alpha.sin() * beta.cos();
    let (p, q, r) = state
        .angular_velocity
        .as_ref()
        .filter(|omega| omega.x.is_finite() && omega.y.is_finite() && omega.z.is_finite())
        .map_or((0.0, 0.0, 0.0), |omega| (omega.x, omega.y, omega.z));

    Some(MsfsAirData {
        true_airspeed_mps: tas,
        velocity_body_x_mps: v_right,
        velocity_body_y_mps: -w_down,
        velocity_body_z_mps: u_forward,
        rotation_body_x_radps: q,
        rotation_body_y_radps: -r,
        rotation_body_z_radps: p,
    })
}

fn engines_from_state(state: &pb::AircraftState) -> EngineThrottles {
    let mut out = EngineThrottles::default();
    for engine in &state.engines {
        if !(1..=4).contains(&engine.index) {
            continue;
        }
        if let Some(value) = valid_ratio(engine.throttle_lever_ratio) {
            out.ratios[engine.index as usize - 1] = Some(value);
        }
    }
    out
}

fn control_values(controls: ControlSurfaces) -> impl Iterator<Item = (Surface, f64)> {
    [
        (Surface::AileronLeft, controls.aileron_left_rad),
        (Surface::AileronRight, controls.aileron_right_rad),
        (Surface::Elevator, controls.elevator_rad),
        (Surface::Rudder, controls.rudder_rad),
        (Surface::FlapsLeft, controls.flaps_left_ratio),
        (Surface::FlapsRight, controls.flaps_right_ratio),
        (Surface::Spoilers, controls.spoilers_ratio),
    ]
    .into_iter()
    .filter_map(|(surface, value)| value.map(|value| (surface, value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fly_ruler_proto_core::Event;

    fn state_with_yaw(yaw: f64) -> pb::AircraftState {
        pb::AircraftState {
            position: None,
            velocity: None,
            attitude: Some(pb::Quaternion {
                w: (yaw * 0.5).cos(),
                x: 0.0,
                y: 0.0,
                z: (yaw * 0.5).sin(),
            }),
            angular_velocity: None,
            derived: Some(pb::DerivedState {
                lat: 31.2,
                lon: 121.5,
                altitude: 1200.0,
                ..Default::default()
            }),
            control_surfaces: None,
            engines: vec![],
            custom_fields: vec![],
        }
    }

    #[test]
    fn converts_ned_yaw_and_normalizes_heading() {
        let frame = frame_from_state(&state_with_yaw(-0.5)).unwrap();
        assert!((frame.pose.heading_true_rad - (TAU - 0.5)).abs() < 1e-12);
        assert_eq!(frame.pose.pitch_rad, -0.0);
        assert_eq!(frame.pose.bank_rad, -0.0);
    }

    #[test]
    fn normalizes_quaternion_and_maps_roll_pitch_signs() {
        let roll = 0.2_f64;
        let pitch = 0.1_f64;
        let (cr, sr) = ((roll / 2.0).cos(), (roll / 2.0).sin());
        let (cp, sp) = ((pitch / 2.0).cos(), (pitch / 2.0).sin());
        let mut state = state_with_yaw(0.0);
        state.attitude = Some(pb::Quaternion {
            w: 2.0 * cr * cp,
            x: 2.0 * sr * cp,
            y: 2.0 * cr * sp,
            z: -2.0 * sr * sp,
        });
        let pose = frame_from_state(&state).unwrap().pose;
        assert!((pose.bank_rad + roll).abs() < 1e-12);
        assert!((pose.pitch_rad + pitch).abs() < 1e-12);
    }

    #[test]
    fn validates_navigation_and_attitude() {
        let mut state = state_with_yaw(0.0);
        state.derived.as_mut().unwrap().lat = 91.0;
        assert_eq!(frame_from_state(&state), Err(FrameError::LatitudeRange));
        state.derived.as_mut().unwrap().lat = 0.0;
        state.attitude = Some(pb::Quaternion::default());
        assert_eq!(frame_from_state(&state), Err(FrameError::ZeroQuaternion));
    }

    #[test]
    fn maps_valid_controls_and_ignores_invalid_ratios() {
        let mut state = state_with_yaw(0.0);
        state.custom_fields = vec![
            custom(fields::RUDDER, 0.12),
            custom(fields::FLAPS_LEFT, 0.4),
            custom(fields::SPOILERS, 1.5),
        ];
        let controls = frame_from_state(&state).unwrap().controls;
        assert_eq!(controls.rudder_rad, Some(0.12));
        assert_eq!(controls.flaps_left_ratio, Some(0.4));
        assert_eq!(controls.spoilers_ratio, None);
    }

    #[test]
    fn standard_controls_override_legacy_fields() {
        let mut state = state_with_yaw(0.0);
        state.custom_fields = vec![
            custom(fields::RUDDER, 0.12),
            custom(fields::FLAPS_LEFT, 0.4),
            custom(fields::SPOILERS, 0.3),
        ];
        state.control_surfaces = Some(pb::ControlSurfaceState {
            rudder_rad: Some(-0.2),
            flaps_left_ratio: Some(0.6),
            spoilers_ratio: Some(1.5),
            ..Default::default()
        });
        let controls = frame_from_state(&state).unwrap().controls;
        assert_eq!(controls.rudder_rad, Some(-0.2));
        assert_eq!(controls.flaps_left_ratio, Some(0.6));
        assert_eq!(controls.spoilers_ratio, None);
    }

    #[test]
    fn reconstructs_airdata_and_maps_body_axes() {
        let mut state = state_with_yaw(0.0);
        let derived = state.derived.as_mut().unwrap();
        derived.tas = 100.0;
        derived.alpha = 0.1;
        derived.beta = -0.2;
        state.angular_velocity = Some(pb::Vector3 {
            x: 0.3,
            y: -0.4,
            z: 0.5,
        });

        let airdata = frame_from_state(&state).unwrap().airdata.unwrap();
        assert!((airdata.velocity_body_x_mps - 100.0 * (-0.2_f64).sin()).abs() < 1e-12);
        assert!(
            (airdata.velocity_body_y_mps + 100.0 * 0.1_f64.sin() * (-0.2_f64).cos()).abs() < 1e-12
        );
        assert!(
            (airdata.velocity_body_z_mps - 100.0 * 0.1_f64.cos() * (-0.2_f64).cos()).abs() < 1e-12
        );
        assert_eq!(airdata.rotation_body_x_radps, -0.4);
        assert_eq!(airdata.rotation_body_y_radps, -0.5);
        assert_eq!(airdata.rotation_body_z_radps, 0.3);
    }

    #[test]
    fn maps_valid_engine_throttles_and_isolates_invalid_entries() {
        let mut state = state_with_yaw(0.0);
        state.engines = vec![
            pb::EngineState {
                index: 1,
                throttle_lever_ratio: Some(0.25),
            },
            pb::EngineState {
                index: 2,
                throttle_lever_ratio: Some(0.75),
            },
            pb::EngineState {
                index: 3,
                throttle_lever_ratio: Some(1.5),
            },
            pb::EngineState {
                index: 5,
                throttle_lever_ratio: Some(0.5),
            },
        ];
        assert_eq!(
            frame_from_state(&state).unwrap().engines.ratios,
            [Some(0.25), Some(0.75), None, None]
        );
        assert_eq!(
            optional_field_warnings(&state),
            [
                "invalid engine throttle ratio",
                "invalid engine index; expected 1 through 4"
            ]
        );
    }

    fn custom(name: &str, value: f64) -> pb::CustomField {
        pb::CustomField {
            field_id: name.to_owned(),
            value: Some(pb::FieldValue {
                kind: Some(pb::field_value::Kind::F64Value(value)),
            }),
        }
    }

    #[derive(Default)]
    struct MockSimulator {
        calls: Vec<String>,
    }

    impl Simulator for MockSimulator {
        type Error = std::convert::Infallible;

        fn set_frozen(&mut self, frozen: bool) -> Result<(), Self::Error> {
            self.calls.push(format!("freeze:{frozen}"));
            Ok(())
        }

        fn set_pose(&mut self, _pose: MsfsPose) -> Result<(), Self::Error> {
            self.calls.push("pose".to_owned());
            Ok(())
        }

        fn set_surface(&mut self, surface: Surface, _value: f64) -> Result<(), Self::Error> {
            self.calls.push(format!("surface:{surface:?}"));
            Ok(())
        }

        fn set_airdata(&mut self, _airdata: MsfsAirData) -> Result<(), Self::Error> {
            self.calls.push("airdata".to_owned());
            Ok(())
        }

        fn set_engine_throttle(&mut self, index: u32, _ratio: f64) -> Result<(), Self::Error> {
            self.calls.push(format!("engine:{index}"));
            Ok(())
        }
    }

    #[test]
    fn session_freezes_before_writes_and_releases_once() {
        let mut state = state_with_yaw(0.0);
        state.custom_fields.push(custom(fields::ELEVATOR, 0.1));
        state.engines.push(pb::EngineState {
            index: 2,
            throttle_lever_ratio: Some(0.7),
        });
        let frame = frame_from_state(&state).unwrap();
        let mut session = BridgeSession::new(MockSimulator::default());
        session.apply(frame).unwrap();
        session.apply(frame).unwrap();
        session.release().unwrap();
        session.release().unwrap();
        assert_eq!(
            session.simulator_mut().calls,
            [
                "freeze:true",
                "pose",
                "airdata",
                "surface:Elevator",
                "engine:2",
                "pose",
                "airdata",
                "surface:Elevator",
                "engine:2",
                "freeze:false",
            ]
        );
    }

    #[test]
    fn selection_prefers_first_active_spawn_and_honors_requested_id() {
        let store = TimeSeriesStore::new();
        spawn(&store, "later", 2.0);
        spawn(&store, "first", 1.0);
        store.append_event("first".to_owned(), 3.0, Event::Custom("tick".to_owned()));

        assert_eq!(select_aircraft(&store, None).as_deref(), Some("first"));
        assert_eq!(
            select_aircraft(&store, Some("later")).as_deref(),
            Some("later")
        );
        assert_eq!(select_aircraft(&store, Some("missing")), None);

        store.append_event(
            "first".to_owned(),
            4.0,
            Event::Despawn(pb::DespawnInfo::default()),
        );
        assert!(!is_spawned(&store, "first"));
        assert_eq!(select_aircraft(&store, None).as_deref(), Some("later"));
    }

    fn spawn(store: &TimeSeriesStore, id: &str, timestamp: f64) {
        store.append_event(
            id.to_owned(),
            timestamp,
            Event::Spawn(Box::new(pb::AircraftSpawnInfo {
                name: id.to_owned(),
                ..Default::default()
            })),
        );
    }
}
