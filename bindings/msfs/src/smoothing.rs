//! Low-latency live sample smoothing for the MSFS bridge.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use fly_ruler_proto_core::pb;

/// Fixed bridge-side live smoothing presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothingMode {
    /// Preserve the old latest-sample behavior for A/B testing.
    Latest,
    /// Small interpolation delay with short extrapolation for hand-flown control.
    LowLatency,
    /// More buffered output for the smoothest visual replay-like rendering.
    Smooth,
}

impl SmoothingMode {
    /// Parse the public CLI/TOML string form.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "latest" => Some(Self::Latest),
            "low_latency" => Some(Self::LowLatency),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }

    /// Return the public CLI/TOML string form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::LowLatency => "low_latency",
            Self::Smooth => "smooth",
        }
    }

    /// Default interpolation delay for this preset.
    pub fn default_interpolation_delay(self) -> Duration {
        match self {
            Self::Latest => Duration::ZERO,
            Self::LowLatency => Duration::from_millis(30),
            Self::Smooth => Duration::from_millis(80),
        }
    }

    /// Default extrapolation limit for this preset.
    pub fn default_max_extrapolation(self) -> Duration {
        match self {
            Self::Latest => Duration::ZERO,
            Self::LowLatency => Duration::from_millis(40),
            Self::Smooth => Duration::from_millis(20),
        }
    }
}

/// Configuration for live smoothing.
#[derive(Debug, Clone)]
pub struct LiveSmoothingConfig {
    /// Smoothing preset.
    pub mode: SmoothingMode,
    /// Source-time delay applied before interpolation.
    pub interpolation_delay: Duration,
    /// Maximum short extrapolation beyond the latest sample.
    pub max_extrapolation: Duration,
    /// Maximum age retained in the ring buffer.
    pub max_buffer_age: Duration,
    /// Maximum samples retained in the ring buffer.
    pub max_samples: usize,
}

impl Default for LiveSmoothingConfig {
    fn default() -> Self {
        let mode = SmoothingMode::LowLatency;
        Self {
            mode,
            interpolation_delay: mode.default_interpolation_delay(),
            max_extrapolation: mode.default_max_extrapolation(),
            max_buffer_age: Duration::from_secs(2),
            max_samples: 512,
        }
    }
}

/// Result of pushing a raw sample into the live buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushResult {
    /// A newer timestamp was appended.
    Accepted,
    /// The latest sample with the same timestamp was replaced.
    UpdatedDuplicate,
    /// The sample timestamp was not finite.
    DroppedInvalidTimestamp,
    /// The sample was older than the newest buffered sample.
    DroppedOutOfOrder,
}

/// Source of a rendered live frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSource {
    /// The latest raw state was used without smoothing.
    Latest,
    /// The frame was interpolated between two raw samples.
    Interpolated,
    /// The frame was extrapolated a short distance beyond the newest sample.
    Extrapolated,
    /// The newest available state was held.
    Held,
}

/// Frame produced by live smoothing.
#[derive(Debug, Clone)]
pub struct SmoothedFrame {
    /// Source timestamp represented by the frame.
    pub timestamp_secs: f64,
    /// Interpolated, extrapolated, or held aircraft state.
    pub state: pb::AircraftState,
    /// How this frame was produced.
    pub source: FrameSource,
}

/// Monotonic counters useful for low-frequency diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SmoothingStats {
    /// Raw samples accepted or same-timestamp updated.
    pub accepted_samples: u64,
    /// Raw samples dropped because they were older than the latest sample.
    pub dropped_out_of_order: u64,
    /// Raw samples dropped because their timestamp was invalid.
    pub dropped_invalid_timestamp: u64,
    /// Rendered latest frames.
    pub latest_frames: u64,
    /// Rendered interpolated frames.
    pub interpolated_frames: u64,
    /// Rendered extrapolated frames.
    pub extrapolated_frames: u64,
    /// Rendered held frames.
    pub held_frames: u64,
}

#[derive(Debug, Clone)]
struct BufferedSample {
    timestamp_secs: f64,
    received_at: Instant,
    state: pb::AircraftState,
}

/// Ring buffer that converts jittery live samples into coherent render frames.
#[derive(Debug, Clone)]
pub struct LiveFrameBuffer {
    config: LiveSmoothingConfig,
    samples: VecDeque<BufferedSample>,
    stats: SmoothingStats,
}

impl LiveFrameBuffer {
    /// Create an empty live frame buffer.
    pub fn new(config: LiveSmoothingConfig) -> Self {
        Self {
            config,
            samples: VecDeque::new(),
            stats: SmoothingStats::default(),
        }
    }

    /// Remove all buffered samples and reset counters.
    pub fn reset(&mut self) {
        self.samples.clear();
        self.stats = SmoothingStats::default();
    }

    /// Return the number of buffered raw samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Return whether no raw samples are buffered.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Return monotonic smoothing counters.
    pub fn stats(&self) -> SmoothingStats {
        self.stats
    }

    /// Push one raw state sample.
    pub fn push(
        &mut self,
        timestamp_secs: f64,
        state: pb::AircraftState,
        received_at: Instant,
    ) -> PushResult {
        if !timestamp_secs.is_finite() {
            self.stats.dropped_invalid_timestamp += 1;
            return PushResult::DroppedInvalidTimestamp;
        }

        match self.samples.back_mut() {
            Some(latest) if timestamp_secs < latest.timestamp_secs => {
                self.stats.dropped_out_of_order += 1;
                PushResult::DroppedOutOfOrder
            }
            Some(latest) if timestamp_secs == latest.timestamp_secs => {
                latest.state = state;
                latest.received_at = received_at;
                self.stats.accepted_samples += 1;
                PushResult::UpdatedDuplicate
            }
            _ => {
                self.samples.push_back(BufferedSample {
                    timestamp_secs,
                    received_at,
                    state,
                });
                self.prune();
                self.stats.accepted_samples += 1;
                PushResult::Accepted
            }
        }
    }

    /// Render the current live frame for the supplied bridge clock instant.
    pub fn render(&mut self, now: Instant) -> Option<SmoothedFrame> {
        let frame = match self.config.mode {
            SmoothingMode::Latest => self.latest_frame(),
            SmoothingMode::LowLatency | SmoothingMode::Smooth => self.smoothed_frame(now),
        }?;
        match frame.source {
            FrameSource::Latest => self.stats.latest_frames += 1,
            FrameSource::Interpolated => self.stats.interpolated_frames += 1,
            FrameSource::Extrapolated => self.stats.extrapolated_frames += 1,
            FrameSource::Held => self.stats.held_frames += 1,
        }
        Some(frame)
    }

    fn latest_frame(&self) -> Option<SmoothedFrame> {
        let sample = self.samples.back()?;
        Some(SmoothedFrame {
            timestamp_secs: sample.timestamp_secs,
            state: sample.state.clone(),
            source: FrameSource::Latest,
        })
    }

    fn smoothed_frame(&self, now: Instant) -> Option<SmoothedFrame> {
        let latest = self.samples.back()?;
        let source_now = latest.timestamp_secs + elapsed_secs(now, latest.received_at);
        let target = source_now - self.config.interpolation_delay.as_secs_f64();

        if self.samples.len() == 1 {
            return Some(SmoothedFrame {
                timestamp_secs: latest.timestamp_secs,
                state: latest.state.clone(),
                source: FrameSource::Held,
            });
        }

        let first = self.samples.front()?;
        if target <= first.timestamp_secs {
            return Some(SmoothedFrame {
                timestamp_secs: first.timestamp_secs,
                state: first.state.clone(),
                source: FrameSource::Held,
            });
        }

        if target > latest.timestamp_secs {
            let beyond = target - latest.timestamp_secs;
            if beyond <= self.config.max_extrapolation.as_secs_f64() {
                let previous = self.samples.get(self.samples.len() - 2)?;
                let state = interpolate_state(previous, latest, target);
                return Some(SmoothedFrame {
                    timestamp_secs: target,
                    state,
                    source: FrameSource::Extrapolated,
                });
            }
            return Some(SmoothedFrame {
                timestamp_secs: latest.timestamp_secs,
                state: latest.state.clone(),
                source: FrameSource::Held,
            });
        }

        for index in 1..self.samples.len() {
            let right = &self.samples[index];
            if target <= right.timestamp_secs {
                let left = &self.samples[index - 1];
                if target == right.timestamp_secs {
                    return Some(SmoothedFrame {
                        timestamp_secs: right.timestamp_secs,
                        state: right.state.clone(),
                        source: FrameSource::Interpolated,
                    });
                }
                return Some(SmoothedFrame {
                    timestamp_secs: target,
                    state: interpolate_state(left, right, target),
                    source: FrameSource::Interpolated,
                });
            }
        }

        Some(SmoothedFrame {
            timestamp_secs: latest.timestamp_secs,
            state: latest.state.clone(),
            source: FrameSource::Held,
        })
    }

    fn prune(&mut self) {
        while self.samples.len() > self.config.max_samples {
            self.samples.pop_front();
        }
        let Some(latest) = self.samples.back() else {
            return;
        };
        let min_timestamp = latest.timestamp_secs - self.config.max_buffer_age.as_secs_f64();
        while self
            .samples
            .front()
            .is_some_and(|sample| sample.timestamp_secs < min_timestamp)
            && self.samples.len() > 2
        {
            self.samples.pop_front();
        }
    }
}

fn elapsed_secs(now: Instant, then: Instant) -> f64 {
    now.saturating_duration_since(then).as_secs_f64()
}

fn interpolate_state(
    left: &BufferedSample,
    right: &BufferedSample,
    target: f64,
) -> pb::AircraftState {
    let span = right.timestamp_secs - left.timestamp_secs;
    let t = if span.abs() <= f64::EPSILON {
        1.0
    } else {
        ((target - left.timestamp_secs) / span).clamp(-1.0, 2.0)
    };

    let mut state = right.state.clone();
    state.position = interpolate_vector(
        left.state.position.as_ref(),
        right.state.position.as_ref(),
        t,
    );
    state.velocity = interpolate_vector(
        left.state.velocity.as_ref(),
        right.state.velocity.as_ref(),
        t,
    );
    state.angular_velocity = interpolate_vector(
        left.state.angular_velocity.as_ref(),
        right.state.angular_velocity.as_ref(),
        t,
    );
    state.attitude = interpolate_quaternion(
        left.state.attitude.as_ref(),
        right.state.attitude.as_ref(),
        t,
    );
    state.derived =
        interpolate_derived(left.state.derived.as_ref(), right.state.derived.as_ref(), t);
    state.control_surfaces = interpolate_controls(
        left.state.control_surfaces.as_ref(),
        right.state.control_surfaces.as_ref(),
        t,
    );
    state.engines = interpolate_engines(&left.state.engines, &right.state.engines, t);
    state.custom_fields =
        interpolate_custom_fields(&left.state.custom_fields, &right.state.custom_fields, t);
    state
}

fn interpolate_vector(
    left: Option<&pb::Vector3>,
    right: Option<&pb::Vector3>,
    t: f64,
) -> Option<pb::Vector3> {
    match (left, right) {
        (Some(left), Some(right)) if finite_vector(left) && finite_vector(right) => {
            Some(pb::Vector3 {
                x: lerp(left.x, right.x, t),
                y: lerp(left.y, right.y, t),
                z: lerp(left.z, right.z, t),
            })
        }
        (_, Some(right)) => Some(*right),
        (Some(left), None) => Some(*left),
        (None, None) => None,
    }
}

fn interpolate_quaternion(
    left: Option<&pb::Quaternion>,
    right: Option<&pb::Quaternion>,
    t: f64,
) -> Option<pb::Quaternion> {
    match (left, right) {
        (Some(left), Some(right)) if finite_quaternion(left) && finite_quaternion(right) => {
            Some(slerp(*left, *right, t))
        }
        (_, Some(right)) => Some(*right),
        (Some(left), None) => Some(*left),
        (None, None) => None,
    }
}

fn interpolate_derived(
    left: Option<&pb::DerivedState>,
    right: Option<&pb::DerivedState>,
    t: f64,
) -> Option<pb::DerivedState> {
    match (left, right) {
        (Some(left), Some(right)) => Some(pb::DerivedState {
            lat: finite_lerp_or_nearest(left.lat, right.lat, t),
            lon: finite_lerp_or_nearest(left.lon, right.lon, t),
            altitude: finite_lerp_or_nearest(left.altitude, right.altitude, t),
            alpha: finite_lerp_or_nearest(left.alpha, right.alpha, t),
            beta: finite_lerp_or_nearest(left.beta, right.beta, t),
            tas: finite_lerp_or_nearest(left.tas, right.tas, t),
            eas: finite_lerp_or_nearest(left.eas, right.eas, t),
            gamma: finite_lerp_or_nearest(left.gamma, right.gamma, t),
            chi: finite_lerp_or_nearest(left.chi, right.chi, t),
            ias: interpolate_option(left.ias, right.ias, t),
            cas: interpolate_option(left.cas, right.cas, t),
            mach: interpolate_option(left.mach, right.mach, t),
        }),
        (_, Some(right)) => Some(*right),
        (Some(left), None) => Some(*left),
        (None, None) => None,
    }
}

fn interpolate_controls(
    left: Option<&pb::ControlSurfaceState>,
    right: Option<&pb::ControlSurfaceState>,
    t: f64,
) -> Option<pb::ControlSurfaceState> {
    match (left, right) {
        (Some(left), Some(right)) => Some(pb::ControlSurfaceState {
            aileron_left_rad: interpolate_option(left.aileron_left_rad, right.aileron_left_rad, t),
            aileron_right_rad: interpolate_option(
                left.aileron_right_rad,
                right.aileron_right_rad,
                t,
            ),
            elevator_rad: interpolate_option(left.elevator_rad, right.elevator_rad, t),
            rudder_rad: interpolate_option(left.rudder_rad, right.rudder_rad, t),
            flaps_left_ratio: interpolate_option(left.flaps_left_ratio, right.flaps_left_ratio, t),
            flaps_right_ratio: interpolate_option(
                left.flaps_right_ratio,
                right.flaps_right_ratio,
                t,
            ),
            spoilers_ratio: interpolate_option(left.spoilers_ratio, right.spoilers_ratio, t),
        }),
        (_, Some(right)) => Some(*right),
        (Some(left), None) => Some(*left),
        (None, None) => None,
    }
}

fn interpolate_engines(
    left: &[pb::EngineState],
    right: &[pb::EngineState],
    t: f64,
) -> Vec<pb::EngineState> {
    let mut engines = right.to_vec();
    for index in 1..=4 {
        let left_value = left
            .iter()
            .find(|engine| engine.index == index)
            .and_then(|engine| engine.throttle_lever_ratio);
        let right_value = right
            .iter()
            .find(|engine| engine.index == index)
            .and_then(|engine| engine.throttle_lever_ratio);
        let value = interpolate_option(left_value, right_value, t);
        let Some(value) = value else {
            continue;
        };
        if let Some(engine) = engines.iter_mut().find(|engine| engine.index == index) {
            engine.throttle_lever_ratio = Some(value);
        } else {
            engines.push(pb::EngineState {
                index,
                throttle_lever_ratio: Some(value),
            });
        }
    }
    engines.sort_by_key(|engine| engine.index);
    engines
}

fn interpolate_custom_fields(
    left: &[pb::CustomField],
    right: &[pb::CustomField],
    t: f64,
) -> Vec<pb::CustomField> {
    let mut fields = right.to_vec();
    for field_id in [
        crate::fields::AILERON_LEFT,
        crate::fields::AILERON_RIGHT,
        crate::fields::ELEVATOR,
        crate::fields::RUDDER,
        crate::fields::FLAPS_LEFT,
        crate::fields::FLAPS_RIGHT,
        crate::fields::SPOILERS,
    ] {
        let left_value = left.iter().find_map(|field| {
            (field.field_id == field_id)
                .then(|| field_f64(field))
                .flatten()
        });
        let right_value = right.iter().find_map(|field| {
            (field.field_id == field_id)
                .then(|| field_f64(field))
                .flatten()
        });
        let Some(value) = interpolate_option(left_value, right_value, t) else {
            continue;
        };
        if let Some(field) = fields.iter_mut().find(|field| field.field_id == field_id) {
            field.value = Some(f64_field_value(value));
        } else {
            fields.push(pb::CustomField {
                field_id: field_id.to_owned(),
                value: Some(f64_field_value(value)),
            });
        }
    }
    fields
}

fn field_f64(field: &pb::CustomField) -> Option<f64> {
    match field.value.as_ref()?.kind.as_ref()? {
        pb::field_value::Kind::F64Value(value) if value.is_finite() => Some(*value),
        pb::field_value::Kind::I64Value(value) => Some(*value as f64),
        _ => None,
    }
}

fn f64_field_value(value: f64) -> pb::FieldValue {
    pb::FieldValue {
        kind: Some(pb::field_value::Kind::F64Value(value)),
    }
}

fn interpolate_option(left: Option<f64>, right: Option<f64>, t: f64) -> Option<f64> {
    match (
        left.filter(|value| value.is_finite()),
        right.filter(|value| value.is_finite()),
    ) {
        (Some(left), Some(right)) => Some(lerp(left, right, t)),
        (_, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn finite_lerp_or_nearest(left: f64, right: f64, t: f64) -> f64 {
    if left.is_finite() && right.is_finite() {
        lerp(left, right, t)
    } else if right.is_finite() {
        right
    } else {
        left
    }
}

fn lerp(left: f64, right: f64, t: f64) -> f64 {
    left + (right - left) * t
}

fn finite_vector(value: &pb::Vector3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn finite_quaternion(value: &pb::Quaternion) -> bool {
    value.w.is_finite() && value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn slerp(mut left: pb::Quaternion, mut right: pb::Quaternion, t: f64) -> pb::Quaternion {
    normalize_quaternion(&mut left);
    normalize_quaternion(&mut right);
    let mut dot = left.w * right.w + left.x * right.x + left.y * right.y + left.z * right.z;
    if dot < 0.0 {
        right.w = -right.w;
        right.x = -right.x;
        right.y = -right.y;
        right.z = -right.z;
        dot = -dot;
    }

    let mut out = if dot > 0.9995 {
        pb::Quaternion {
            w: lerp(left.w, right.w, t),
            x: lerp(left.x, right.x, t),
            y: lerp(left.y, right.y, t),
            z: lerp(left.z, right.z, t),
        }
    } else {
        let theta_0 = dot.clamp(-1.0, 1.0).acos();
        let sin_theta_0 = theta_0.sin();
        let theta = theta_0 * t;
        let sin_theta = theta.sin();
        let scale_left = (theta_0 - theta).sin() / sin_theta_0;
        let scale_right = sin_theta / sin_theta_0;
        pb::Quaternion {
            w: scale_left * left.w + scale_right * right.w,
            x: scale_left * left.x + scale_right * right.x,
            y: scale_left * left.y + scale_right * right.y,
            z: scale_left * left.z + scale_right * right.z,
        }
    };
    normalize_quaternion(&mut out);
    out
}

fn normalize_quaternion(value: &mut pb::Quaternion) {
    let norm =
        (value.w * value.w + value.x * value.x + value.y * value.y + value.z * value.z).sqrt();
    if norm > f64::EPSILON {
        value.w /= norm;
        value.x /= norm;
        value.y /= norm;
        value.z /= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frame_from_state, GearCommand, MsfsPose, Simulator, Surface};

    fn state(timestamp_index: f64, yaw: f64) -> pb::AircraftState {
        pb::AircraftState {
            velocity: Some(pb::Vector3 {
                x: 100.0 + timestamp_index,
                y: -5.0 + timestamp_index,
                z: 2.0 * timestamp_index,
            }),
            attitude: Some(pb::Quaternion {
                w: (yaw * 0.5).cos(),
                x: 0.0,
                y: 0.0,
                z: (yaw * 0.5).sin(),
            }),
            angular_velocity: Some(pb::Vector3 {
                x: 0.1 * timestamp_index,
                y: 0.2 * timestamp_index,
                z: 0.3 * timestamp_index,
            }),
            derived: Some(pb::DerivedState {
                lat: 10.0 + timestamp_index,
                lon: 20.0 + timestamp_index,
                altitude: 1000.0 + 10.0 * timestamp_index,
                alpha: 0.1 * timestamp_index,
                beta: 0.01 * timestamp_index,
                tas: 100.0 + timestamp_index,
                ..Default::default()
            }),
            control_surfaces: Some(pb::ControlSurfaceState {
                elevator_rad: Some(0.1 * timestamp_index),
                spoilers_ratio: Some(0.5),
                ..Default::default()
            }),
            engines: vec![pb::EngineState {
                index: 1,
                throttle_lever_ratio: Some(0.2 + 0.1 * timestamp_index),
            }],
            ..Default::default()
        }
    }

    fn low_latency() -> LiveSmoothingConfig {
        LiveSmoothingConfig {
            mode: SmoothingMode::LowLatency,
            interpolation_delay: Duration::from_millis(30),
            max_extrapolation: Duration::from_millis(40),
            max_buffer_age: Duration::from_secs(2),
            max_samples: 512,
        }
    }

    #[test]
    fn buffer_deduplicates_drops_out_of_order_and_trims_capacity() {
        let start = Instant::now();
        let mut buffer = LiveFrameBuffer::new(LiveSmoothingConfig {
            max_samples: 2,
            ..low_latency()
        });
        assert_eq!(
            buffer.push(1.0, state(1.0, 0.0), start),
            PushResult::Accepted
        );
        assert_eq!(
            buffer.push(1.0, state(2.0, 0.0), start),
            PushResult::UpdatedDuplicate
        );
        assert_eq!(
            buffer.push(0.5, state(0.5, 0.0), start),
            PushResult::DroppedOutOfOrder
        );
        assert_eq!(
            buffer.push(f64::NAN, state(0.0, 0.0), start),
            PushResult::DroppedInvalidTimestamp
        );
        assert_eq!(
            buffer.push(2.0, state(2.0, 0.0), start),
            PushResult::Accepted
        );
        assert_eq!(
            buffer.push(3.0, state(3.0, 0.0), start),
            PushResult::Accepted
        );
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.stats().dropped_out_of_order, 1);
        assert_eq!(buffer.stats().dropped_invalid_timestamp, 1);
    }

    #[test]
    fn renders_interpolated_frame_at_low_latency_delay() {
        let start = Instant::now();
        let mut buffer = LiveFrameBuffer::new(low_latency());
        buffer.push(10.00, state(0.0, 0.0), start);
        buffer.push(10.10, state(1.0, 0.1), start + Duration::from_millis(100));

        let frame = buffer.render(start + Duration::from_millis(120)).unwrap();
        assert_eq!(frame.source, FrameSource::Interpolated);
        assert!((frame.timestamp_secs - 10.09).abs() < 1e-9);
        let derived = frame.state.derived.unwrap();
        assert!((derived.lat - 10.9).abs() < 1e-9);
        assert!((derived.altitude - 1009.0).abs() < 1e-9);
        let controls = frame.state.control_surfaces.unwrap();
        assert!((controls.elevator_rad.unwrap() - 0.09).abs() < 1e-9);
        assert!((frame.state.engines[0].throttle_lever_ratio.unwrap() - 0.29).abs() < 1e-9);
    }

    #[test]
    fn extrapolates_short_gap_then_holds() {
        let start = Instant::now();
        let mut buffer = LiveFrameBuffer::new(low_latency());
        buffer.push(10.00, state(0.0, 0.0), start);
        buffer.push(10.10, state(1.0, 0.1), start + Duration::from_millis(100));

        let extrapolated = buffer.render(start + Duration::from_millis(160)).unwrap();
        assert_eq!(extrapolated.source, FrameSource::Extrapolated);
        assert!((extrapolated.timestamp_secs - 10.13).abs() < 1e-9);

        let held = buffer.render(start + Duration::from_millis(180)).unwrap();
        assert_eq!(held.source, FrameSource::Held);
        assert!((held.timestamp_secs - 10.10).abs() < 1e-9);
    }

    #[test]
    fn latest_mode_uses_newest_raw_state() {
        let start = Instant::now();
        let mut buffer = LiveFrameBuffer::new(LiveSmoothingConfig {
            mode: SmoothingMode::Latest,
            ..low_latency()
        });
        buffer.push(1.0, state(1.0, 0.0), start);
        buffer.push(2.0, state(2.0, 0.0), start);
        let frame = buffer.render(start).unwrap();
        assert_eq!(frame.source, FrameSource::Latest);
        assert_eq!(frame.timestamp_secs, 2.0);
        assert_eq!(frame.state.derived.unwrap().lat, 12.0);
    }

    #[test]
    fn slerp_handles_heading_wrap_without_zero_crossing_jump() {
        let start = Instant::now();
        let mut buffer = LiveFrameBuffer::new(low_latency());
        buffer.push(10.00, state(0.0, 6.20), start);
        buffer.push(10.10, state(1.0, 0.05), start + Duration::from_millis(100));

        let frame = buffer.render(start + Duration::from_millis(80)).unwrap();
        let heading = frame_from_state(&frame.state)
            .unwrap()
            .pose
            .heading_true_rad;
        assert!(!(0.05..=6.20).contains(&heading));
    }

    #[derive(Default)]
    struct MockSimulator {
        calls: Vec<&'static str>,
    }

    impl Simulator for MockSimulator {
        type Error = std::convert::Infallible;

        fn set_frozen(&mut self, _frozen: bool) -> Result<(), Self::Error> {
            self.calls.push("freeze");
            Ok(())
        }

        fn set_pose(&mut self, _pose: MsfsPose) -> Result<(), Self::Error> {
            self.calls.push("pose");
            Ok(())
        }

        fn set_surface(&mut self, _surface: Surface, _value: f64) -> Result<(), Self::Error> {
            self.calls.push("surface");
            Ok(())
        }

        fn set_airdata(&mut self, _airdata: crate::MsfsAirData) -> Result<(), Self::Error> {
            self.calls.push("airdata");
            Ok(())
        }

        fn set_engine_throttle(&mut self, _index: u32, _ratio: f64) -> Result<(), Self::Error> {
            self.calls.push("engine");
            Ok(())
        }

        fn set_landing_gear(&mut self, _command: GearCommand) -> Result<(), Self::Error> {
            self.calls.push("gear");
            Ok(())
        }
    }

    #[test]
    fn smoothed_frame_preserves_existing_simconnect_write_order() {
        let mut simulator = crate::BridgeSession::new(MockSimulator::default());
        let frame = frame_from_state(&state(1.0, 0.0)).unwrap();
        simulator.apply(frame).unwrap();
        assert_eq!(
            simulator.simulator_mut().calls,
            ["freeze", "pose", "airdata", "surface", "surface", "engine"]
        );
    }
}
