//! FlyRuler to Microsoft Flight Simulator 2024 bridge logic.

use std::f64::consts::TAU;

use fly_ruler_proto_core::pb;
use fly_ruler_proto_core::{Attitude, AttitudeError, Event, PlaybackMode, TimeSeriesStore};
use thiserror::Error;

#[cfg(windows)]
pub mod simconnect;
pub mod smoothing;

/// Reserved FlyRuler custom events understood by the MSFS bridge.
pub mod events {
    /// Retract the landing gear through the MSFS gear handle.
    pub const GEAR_UP: &str = "flyruler.control.gear_up";
    /// Extend the landing gear through the MSFS gear handle.
    pub const GEAR_DOWN: &str = "flyruler.control.gear_down";
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

/// Body velocity and angular rates expressed in MSFS body axes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct MsfsAirData {
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

/// Optional propulsor throttle positions mapped to simulator slots.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PropulsorThrottles {
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
    pub propulsors: PropulsorThrottles,
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
    /// The attitude is not a valid finite SO(3) value.
    #[error("invalid aircraft attitude: {0}")]
    InvalidAttitude(#[from] AttitudeError),
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

/// Landing-gear handle commands supported by the MSFS bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GearCommand {
    /// Retract the landing gear.
    Up,
    /// Extend the landing gear.
    Down,
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
    /// Write body velocity and angular rates.
    fn set_airdata(&mut self, airdata: MsfsAirData) -> Result<(), Self::Error>;
    /// Write one indexed engine throttle lever position.
    fn set_engine_throttle(&mut self, index: u32, ratio: f64) -> Result<(), Self::Error>;
    /// Move the landing-gear handle to a deterministic position.
    fn set_landing_gear(&mut self, command: GearCommand) -> Result<(), Self::Error>;
}

/// Select an active aircraft at a global replay cursor.
pub fn select_aircraft_at(
    store: &TimeSeriesStore,
    requested: Option<&str>,
    timestamp_secs: f64,
) -> Option<String> {
    if let Some(id) = requested {
        return store
            .is_spawned_at(&id.to_owned(), timestamp_secs)
            .then(|| id.to_owned());
    }

    store
        .get_aircraft_ids()
        .into_iter()
        .filter(|id| store.is_spawned_at(&id.to_owned(), timestamp_secs))
        .filter_map(|id| {
            // first spawn timestamp of each aircraft
            store
                .get_events_range(&id, f64::NEG_INFINITY, f64::INFINITY)?
                .into_iter()
                .find_map(|entry| {
                    matches!(entry.event, Event::Spawn(_)).then_some(entry.timestamp_secs)
                })
                .map(|timestamp| (timestamp, id))
        })
        .filter(|(spawn_timestamp, _)| *spawn_timestamp <= timestamp_secs)
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        })
        .map(|(_, id)| id)
}

/// Tracks landing-gear events across live and replay cursor changes.
#[derive(Debug, Default)]
pub struct GearEventTracker {
    aircraft_id: Option<String>,
    cursor_secs: Option<f64>,
    revision: Option<u64>,
    mode: Option<PlaybackMode>,
    last_command: Option<GearCommand>,
}

impl GearEventTracker {
    /// Reset all event history when an aircraft is released or SimConnect reconnects.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Resolve the gear commands that must be sent for the current playback snapshot.
    ///
    /// Discontinuous cursor changes synchronize the last command at or before
    /// the cursor. Monotonic playback emits every crossed command in order.
    pub fn commands_to_apply(
        &mut self,
        store: &TimeSeriesStore,
        aircraft_id: &str,
        mode: PlaybackMode,
        cursor_secs: f64,
        revision: u64,
    ) -> Vec<GearCommand> {
        if !cursor_secs.is_finite() {
            return Vec::new();
        }

        let discontinuity = self.aircraft_id.as_deref() != Some(aircraft_id)
            || self.cursor_secs.is_none()
            || self.revision != Some(revision)
            || self.mode != Some(mode)
            || self
                .cursor_secs
                .is_some_and(|previous| cursor_secs < previous);

        let mut commands = if discontinuity {
            latest_gear_command(store, aircraft_id, cursor_secs)
                .into_iter()
                .collect()
        } else {
            let previous = self.cursor_secs.unwrap_or(cursor_secs);
            gear_commands_between(store, aircraft_id, previous, cursor_secs)
        };

        if !discontinuity {
            commands.retain(|command| {
                if self.last_command == Some(*command) {
                    false
                } else {
                    self.last_command = Some(*command);
                    true
                }
            });
        } else if let Some(command) = commands.last().copied() {
            self.last_command = Some(command);
        } else {
            self.last_command = None;
        }

        // Live data can append an event whose timestamp does not advance the
        // global cursor. Re-resolve the effective state so it is not missed.
        if mode == PlaybackMode::Live && !discontinuity {
            if let Some(command) = latest_gear_command(store, aircraft_id, cursor_secs) {
                if self.last_command != Some(command) {
                    commands.push(command);
                    self.last_command = Some(command);
                }
            }
        }

        self.aircraft_id = Some(aircraft_id.to_owned());
        self.cursor_secs = Some(cursor_secs);
        self.revision = Some(revision);
        self.mode = Some(mode);
        commands
    }
}

fn latest_gear_command(
    store: &TimeSeriesStore,
    aircraft_id: &str,
    cursor_secs: f64,
) -> Option<GearCommand> {
    store
        .get_events_range(&aircraft_id.to_owned(), f64::NEG_INFINITY, cursor_secs)?
        .into_iter()
        .rev()
        .find_map(|entry| gear_command_from_event(&entry.event))
}

fn gear_commands_between(
    store: &TimeSeriesStore,
    aircraft_id: &str,
    start_exclusive: f64,
    end_inclusive: f64,
) -> Vec<GearCommand> {
    store
        .get_events_range(&aircraft_id.to_owned(), start_exclusive, end_inclusive)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.timestamp_secs > start_exclusive)
        .filter_map(|entry| gear_command_from_event(&entry.event))
        .collect()
}

fn gear_command_from_event(event: &Event) -> Option<GearCommand> {
    match event {
        Event::Custom(name) if name == events::GEAR_UP => Some(GearCommand::Up),
        Event::Custom(name) if name == events::GEAR_DOWN => Some(GearCommand::Down),
        _ => None,
    }
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
        tracing::debug!(
            target: "fly_ruler_proto_msfs.bridge",
            altitude_m = frame.pose.altitude_m,
            pitch_rad = frame.pose.pitch_rad,
            bank_rad = frame.pose.bank_rad,
            heading_true_rad = frame.pose.heading_true_rad,
            velocity_body_x_mps = frame.airdata.map(|value| value.velocity_body_x_mps),
            velocity_body_y_mps = frame.airdata.map(|value| value.velocity_body_y_mps),
            velocity_body_z_mps = frame.airdata.map(|value| value.velocity_body_z_mps),
            "writing MSFS frame"
        );
        self.simulator.set_pose(frame.pose)?;
        if let Some(airdata) = frame.airdata {
            self.simulator.set_airdata(airdata)?;
        }
        for (surface, value) in control_values(frame.controls) {
            self.simulator.set_surface(surface, value)?;
        }
        for (offset, ratio) in frame.propulsors.ratios.into_iter().enumerate() {
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

    /// Apply one landing-gear command without changing the motion freeze state.
    pub fn apply_gear_command(&mut self, command: GearCommand) -> Result<(), S::Error> {
        self.simulator.set_landing_gear(command)
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

    let [roll, pitch, yaw] = Attitude::try_from(attitude)?.euler();

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
        propulsors: propulsors_from_state(state),
    })
}

/// Describe invalid optional fields isolated from an otherwise valid frame.
pub fn optional_field_warnings(state: &pb::AircraftState) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    if state.velocity.is_some() && airdata_from_state(state).is_none() {
        warnings.push("invalid body velocity or angular velocity; airdata was not written");
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

    let mut seen_indices = [false; 4];
    for propulsor in &state.propulsors {
        let Some(index) = propulsor.index else {
            if propulsor.throttle_ratio.is_some() {
                warnings.push("propulsor throttle has no simulator index");
            }
            continue;
        };
        if !(1..=4).contains(&index) {
            warnings.push("invalid propulsor index; expected 1 through 4");
        } else {
            let slot = index as usize - 1;
            if seen_indices[slot] {
                warnings.push("duplicate propulsor index; last value wins");
            }
            seen_indices[slot] = true;
            if propulsor
                .throttle_ratio
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                warnings.push("invalid propulsor throttle ratio");
            }
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

fn valid_finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn valid_ratio(value: Option<f64>) -> Option<f64> {
    valid_finite(value).filter(|value| (0.0..=1.0).contains(value))
}

fn controls_from_state(state: &pb::AircraftState) -> ControlSurfaces {
    let mut out = ControlSurfaces::default();
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
    let velocity = state.velocity.as_ref()?;
    if !velocity.x.is_finite() || !velocity.y.is_finite() || !velocity.z.is_finite() {
        return None;
    }

    let (p, q, r) = state
        .angular_velocity
        .as_ref()
        .filter(|omega| omega.x.is_finite() && omega.y.is_finite() && omega.z.is_finite())
        .map_or((0.0, 0.0, 0.0), |omega| (omega.x, omega.y, omega.z));

    // FlyRuler velocity is body-FRD: X forward, Y right, Z down.
    // MSFS body velocity SimVars use X right, Y up, Z forward.
    Some(MsfsAirData {
        velocity_body_x_mps: velocity.y,
        velocity_body_y_mps: -velocity.z,
        velocity_body_z_mps: velocity.x,
        rotation_body_x_radps: q,
        rotation_body_y_radps: -r,
        rotation_body_z_radps: p,
    })
}

fn propulsors_from_state(state: &pb::AircraftState) -> PropulsorThrottles {
    let mut out = PropulsorThrottles::default();
    for propulsor in &state.propulsors {
        let Some(index) = propulsor.index else {
            continue;
        };
        if !(1..=4).contains(&index) {
            continue;
        }
        if let Some(value) = valid_ratio(propulsor.throttle_ratio) {
            out.ratios[index as usize - 1] = Some(value);
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

/// Iterate over valid optional control-surface writes in a frame.
pub fn frame_control_values(frame: MsfsFrame) -> impl Iterator<Item = (Surface, f64)> {
    control_values(frame.controls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fly_ruler_proto_core::Event;

    /// Select a requested active aircraft or the earliest active spawn.
    fn select_aircraft(store: &TimeSeriesStore, requested: Option<&str>) -> Option<String> {
        select_aircraft_at(store, requested, f64::INFINITY)
    }

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
            linear_acceleration_body: None,
            propulsors: vec![],
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
        assert_eq!(
            frame_from_state(&state),
            Err(FrameError::InvalidAttitude(
                AttitudeError::InvalidQuaternion
            ))
        );
    }

    #[test]
    fn maps_valid_controls_and_ignores_invalid_ratios() {
        let mut state = state_with_yaw(0.0);
        state.control_surfaces = Some(pb::ControlSurfaceState {
            rudder_rad: Some(0.12),
            flaps_left_ratio: Some(0.4),
            spoilers_ratio: Some(1.5),
            ..Default::default()
        });
        let controls = frame_from_state(&state).unwrap().controls;
        assert_eq!(controls.rudder_rad, Some(0.12));
        assert_eq!(controls.flaps_left_ratio, Some(0.4));
        assert_eq!(controls.spoilers_ratio, None);
    }

    #[test]
    fn missing_standard_controls_produce_no_surface_writes() {
        let mut state = state_with_yaw(0.0);
        state.control_surfaces = None;
        let controls = frame_from_state(&state).unwrap().controls;
        assert_eq!(controls.rudder_rad, None);
        assert_eq!(controls.flaps_left_ratio, None);
        assert_eq!(controls.spoilers_ratio, None);
    }

    #[test]
    fn maps_proto_body_velocity_to_msfs_body_axes() {
        let mut state = state_with_yaw(0.0);
        state.velocity = Some(pb::Vector3 {
            x: 101.0,
            y: -12.0,
            z: 5.0,
        });
        state.angular_velocity = Some(pb::Vector3 {
            x: 0.3,
            y: -0.4,
            z: 0.5,
        });

        let airdata = frame_from_state(&state).unwrap().airdata.unwrap();
        assert_eq!(airdata.velocity_body_x_mps, -12.0);
        assert_eq!(airdata.velocity_body_y_mps, -5.0);
        assert_eq!(airdata.velocity_body_z_mps, 101.0);
        assert_eq!(airdata.rotation_body_x_radps, -0.4);
        assert_eq!(airdata.rotation_body_y_radps, -0.5);
        assert_eq!(airdata.rotation_body_z_radps, 0.3);
    }

    #[test]
    fn does_not_reconstruct_airdata_from_alpha_beta_and_tas() {
        let mut state = state_with_yaw(0.0);
        let derived = state.derived.as_mut().unwrap();
        derived.tas = 100.0;
        derived.alpha = 0.1;
        derived.beta = -0.2;
        assert!(frame_from_state(&state).unwrap().airdata.is_none());
    }

    #[test]
    fn maps_valid_propulsor_throttles_and_isolates_invalid_entries() {
        let mut state = state_with_yaw(0.0);
        state.propulsors = vec![
            pb::PropulsorState {
                propulsor_id: "engine.1".to_string(),
                index: Some(1),
                throttle_ratio: Some(0.25),
                ..Default::default()
            },
            pb::PropulsorState {
                propulsor_id: "engine.2".to_string(),
                index: Some(2),
                throttle_ratio: Some(0.75),
                ..Default::default()
            },
            pb::PropulsorState {
                propulsor_id: "engine.3".to_string(),
                index: Some(3),
                throttle_ratio: Some(1.5),
                ..Default::default()
            },
            pb::PropulsorState {
                propulsor_id: "engine.5".to_string(),
                index: Some(5),
                throttle_ratio: Some(0.5),
                ..Default::default()
            },
            pb::PropulsorState {
                propulsor_id: "unmapped".to_string(),
                throttle_ratio: Some(0.4),
                ..Default::default()
            },
            pb::PropulsorState {
                propulsor_id: "engine.2.backup".to_string(),
                index: Some(2),
                throttle_ratio: Some(0.9),
                ..Default::default()
            },
        ];
        assert_eq!(
            frame_from_state(&state).unwrap().propulsors.ratios,
            [Some(0.25), Some(0.9), None, None]
        );
        assert_eq!(
            optional_field_warnings(&state),
            [
                "invalid propulsor throttle ratio",
                "invalid propulsor index; expected 1 through 4",
                "propulsor throttle has no simulator index",
                "duplicate propulsor index; last value wins"
            ]
        );
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

        fn set_landing_gear(&mut self, command: GearCommand) -> Result<(), Self::Error> {
            self.calls.push(format!("gear:{command:?}"));
            Ok(())
        }
    }

    #[test]
    fn session_freezes_before_writes_and_releases_once() {
        let mut state = state_with_yaw(0.0);
        state.velocity = Some(pb::Vector3 {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        });
        state.control_surfaces = Some(pb::ControlSurfaceState {
            elevator_rad: Some(0.1),
            ..Default::default()
        });
        state.propulsors.push(pb::PropulsorState {
            propulsor_id: "engine.2".to_string(),
            index: Some(2),
            throttle_ratio: Some(0.7),
            ..Default::default()
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
        assert!(!store.is_spawned_at(&"first".to_string(), f64::INFINITY));
        assert_eq!(select_aircraft(&store, None).as_deref(), Some("later"));
    }

    #[test]
    fn replay_selection_respects_spawn_and_despawn_times() {
        let store = TimeSeriesStore::new();
        spawn(&store, "first", 1.0);
        spawn(&store, "second", 3.0);
        store.append_event(
            "first".to_owned(),
            4.0,
            Event::Despawn(pb::DespawnInfo::default()),
        );

        assert_eq!(select_aircraft_at(&store, None, 0.5), None);
        assert_eq!(
            select_aircraft_at(&store, None, 2.0).as_deref(),
            Some("first")
        );
        assert_eq!(
            select_aircraft_at(&store, None, 5.0).as_deref(),
            Some("second")
        );
        assert_eq!(select_aircraft_at(&store, Some("first"), 5.0), None);
    }

    #[test]
    fn gear_tracker_maps_only_reserved_custom_events() {
        let store = TimeSeriesStore::new();
        spawn(&store, "a", 1.0);
        store.append_event(
            "a".to_owned(),
            2.0,
            Event::Custom(events::GEAR_UP.to_owned()),
        );
        store.append_event("a".to_owned(), 3.0, Event::Custom("gear_up".to_owned()));
        store.append_event(
            "a".to_owned(),
            4.0,
            Event::Custom(events::GEAR_DOWN.to_owned()),
        );

        let mut tracker = GearEventTracker::default();
        assert_eq!(
            tracker.commands_to_apply(&store, "a", PlaybackMode::ReplayPaused, 2.5, 1),
            [GearCommand::Up]
        );
        assert_eq!(
            tracker.commands_to_apply(&store, "a", PlaybackMode::ReplayPlaying, 4.0, 2),
            [GearCommand::Down]
        );
    }

    #[test]
    fn gear_tracker_emits_crossed_commands_and_deduplicates_live_state() {
        let store = TimeSeriesStore::new();
        spawn(&store, "a", 1.0);
        let mut tracker = GearEventTracker::default();
        assert!(tracker
            .commands_to_apply(&store, "a", PlaybackMode::ReplayPlaying, 1.0, 1)
            .is_empty());

        store.append_event(
            "a".to_owned(),
            2.0,
            Event::Custom(events::GEAR_UP.to_owned()),
        );
        store.append_event(
            "a".to_owned(),
            3.0,
            Event::Custom(events::GEAR_DOWN.to_owned()),
        );
        assert_eq!(
            tracker.commands_to_apply(&store, "a", PlaybackMode::ReplayPlaying, 3.5, 1),
            [GearCommand::Up, GearCommand::Down]
        );

        let mut live = GearEventTracker::default();
        assert_eq!(
            live.commands_to_apply(&store, "a", PlaybackMode::Live, 4.0, 1),
            [GearCommand::Down]
        );
        store.append_event(
            "a".to_owned(),
            4.0,
            Event::Custom(events::GEAR_DOWN.to_owned()),
        );
        assert!(live
            .commands_to_apply(&store, "a", PlaybackMode::Live, 4.0, 1)
            .is_empty());
        store.append_event(
            "a".to_owned(),
            4.0,
            Event::Custom(events::GEAR_UP.to_owned()),
        );
        assert_eq!(
            live.commands_to_apply(&store, "a", PlaybackMode::Live, 4.0, 1),
            [GearCommand::Up]
        );
    }

    #[test]
    fn gear_tracker_resynchronizes_on_seek_and_reset() {
        let store = TimeSeriesStore::new();
        spawn(&store, "a", 1.0);
        store.append_event(
            "a".to_owned(),
            2.0,
            Event::Custom(events::GEAR_UP.to_owned()),
        );
        store.append_event(
            "a".to_owned(),
            4.0,
            Event::Custom(events::GEAR_DOWN.to_owned()),
        );

        let mut tracker = GearEventTracker::default();
        assert_eq!(
            tracker.commands_to_apply(&store, "a", PlaybackMode::ReplayPaused, 5.0, 1),
            [GearCommand::Down]
        );
        assert_eq!(
            tracker.commands_to_apply(&store, "a", PlaybackMode::ReplayPaused, 3.0, 2),
            [GearCommand::Up]
        );
        tracker.reset();
        assert!(tracker
            .commands_to_apply(&store, "a", PlaybackMode::ReplayPaused, 1.5, 3)
            .is_empty());
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
