use std::time::{Duration, Instant};

use fly_ruler_proto_core::{
    pb::AircraftState, PlaybackMode, PlaybackSnapshot, TimeSeriesStore, TimestampedState,
};
use fly_ruler_proto_msfs::simconnect::{
    MsfsObjectId, SimConnectClient, SimConnectError, OBJECT_USER,
};
use fly_ruler_proto_msfs::smoothing::{
    FrameSource, LiveFrameBuffer, LiveSmoothingConfig, PushResult, SmoothingMode, SmoothingStats,
};
use fly_ruler_proto_msfs::{
    frame_control_values, frame_from_state, optional_field_warnings, GearCommand, GearEventTracker,
    MsfsFrame,
};
use tracing::{info, warn};

pub(crate) struct ObjectSession {
    pub(crate) object_id: MsfsObjectId,
    frozen: bool,
    user_aircraft: bool,
}

impl ObjectSession {
    pub(crate) fn user() -> Self {
        Self {
            object_id: OBJECT_USER,
            frozen: false,
            user_aircraft: true,
        }
    }

    pub(crate) fn ai(object_id: MsfsObjectId) -> Self {
        Self {
            object_id,
            frozen: false,
            user_aircraft: false,
        }
    }

    pub(crate) fn apply(
        &mut self,
        simulator: &mut SimConnectClient,
        frame: MsfsFrame,
    ) -> Result<(), SimConnectError> {
        if !self.frozen {
            simulator.set_frozen_for_object(self.object_id, true)?;
            self.frozen = true;
        }
        simulator.set_pose_for_object(self.object_id, frame.pose)?;
        if let Some(airdata) = frame.airdata {
            simulator.set_airdata_for_object(self.object_id, airdata)?;
        }
        for (surface, value) in frame_control_values(frame) {
            simulator.set_surface_for_object(self.object_id, surface, value)?;
        }
        for (offset, ratio) in frame.propulsors.ratios.into_iter().enumerate() {
            if let Some(ratio) = ratio {
                simulator.set_engine_throttle_for_object(
                    self.object_id,
                    offset as u32 + 1,
                    ratio,
                )?;
            }
        }
        Ok(())
    }

    fn apply_gear(
        &mut self,
        simulator: &mut SimConnectClient,
        command: GearCommand,
    ) -> Result<(), SimConnectError> {
        simulator.set_landing_gear_for_object(self.object_id, command)
    }

    pub(crate) fn release(
        &mut self,
        simulator: &mut SimConnectClient,
    ) -> Result<(), SimConnectError> {
        if self.user_aircraft && self.frozen {
            simulator.set_frozen_for_object(self.object_id, false)?;
            self.frozen = false;
        }
        Ok(())
    }
}

pub(crate) struct AircraftRuntime {
    pub(crate) session: ObjectSession,
    smoother: LiveFrameBuffer,
    last_sample_key: Option<(u64, u64)>,
    last_state: Option<AircraftState>,
    stale_reported: bool,
    last_optional_warnings: Vec<&'static str>,
    gear_events: GearEventTracker,
    last_playback_mode: Option<PlaybackMode>,
    last_smoothing_stats: SmoothingStats,
    last_smoothing_log: Instant,
}

impl AircraftRuntime {
    pub(crate) fn new(session: ObjectSession, smoothing: LiveSmoothingConfig) -> Self {
        Self {
            session,
            smoother: LiveFrameBuffer::new(smoothing),
            last_sample_key: None,
            last_state: None,
            stale_reported: false,
            last_optional_warnings: Vec::new(),
            gear_events: GearEventTracker::default(),
            last_playback_mode: None,
            last_smoothing_stats: SmoothingStats::default(),
            last_smoothing_log: Instant::now(),
        }
    }

    pub(crate) fn reset_timeline_state(&mut self) {
        self.smoother.reset();
        self.last_sample_key = None;
        self.last_state = None;
        self.stale_reported = false;
        self.last_optional_warnings.clear();
        self.gear_events.reset();
        self.last_smoothing_stats = SmoothingStats::default();
        self.last_smoothing_log = Instant::now();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn process(
    simulator: &mut SimConnectClient,
    store: &TimeSeriesStore,
    playback_state: &PlaybackSnapshot,
    aircraft_id: &str,
    runtime: &mut AircraftRuntime,
    loop_start: Instant,
    stale_timeout: Duration,
    smoothing_mode: SmoothingMode,
) -> Result<(), SimConnectError> {
    if runtime.last_playback_mode != Some(playback_state.mode) {
        runtime.reset_timeline_state();
        runtime.last_playback_mode = Some(playback_state.mode);
    }

    if let Some(cursor_secs) = playback_state.cursor_secs {
        for command in runtime.gear_events.commands_to_apply(
            store,
            aircraft_id,
            playback_state.mode,
            cursor_secs,
            playback_state.revision,
        ) {
            runtime.session.apply_gear(simulator, command)?;
            info!(
                target: "fly_ruler_proto_msfs.bridge",
                aircraft_id,
                object_id = runtime.session.object_id,
                ?command,
                "applied MSFS landing gear command"
            );
        }
    }

    let Some(sample) = resolve_sample(store, playback_state, aircraft_id) else {
        return Ok(());
    };

    if playback_state.mode == PlaybackMode::Live {
        let live_stale = store.live_state_is_stale(aircraft_id, stale_timeout);
        if !live_stale {
            runtime.stale_reported = false;
        }
        let sample_key = (sample.timestamp_secs.to_bits(), playback_state.revision);
        let raw_changed = runtime.last_sample_key != Some(sample_key)
            || runtime.last_state.as_ref() != Some(&sample.state);
        if raw_changed {
            match runtime
                .smoother
                .push(sample.timestamp_secs, sample.state.clone(), loop_start)
            {
                PushResult::Accepted | PushResult::UpdatedDuplicate => {
                    runtime.stale_reported = false;
                }
                PushResult::DroppedInvalidTimestamp => {
                    warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, "dropped aircraft state with invalid timestamp");
                }
                PushResult::DroppedOutOfOrder => {
                    warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, timestamp_secs = sample.timestamp_secs, "dropped out-of-order aircraft state");
                }
            }
            runtime.last_sample_key = Some(sample_key);
            runtime.last_state = Some(sample.state.clone());
        }

        let should_render = smoothing_mode != SmoothingMode::Latest || raw_changed;
        if should_render {
            if let Some(smoothed) = runtime.smoother.render(loop_start) {
                apply_state(
                    simulator,
                    aircraft_id,
                    runtime,
                    &smoothed.state,
                    Some(smoothed.source),
                    live_stale,
                )?;
            }
        } else if live_stale && !runtime.stale_reported {
            warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, "aircraft state is stale; holding final MSFS pose");
            runtime.stale_reported = true;
        }
    } else {
        let sample_key = (sample.timestamp_secs.to_bits(), playback_state.revision);
        if runtime.last_sample_key != Some(sample_key)
            || runtime.last_state.as_ref() != Some(&sample.state)
        {
            apply_state(simulator, aircraft_id, runtime, &sample.state, None, false)?;
            runtime.last_sample_key = Some(sample_key);
            runtime.last_state = Some(sample.state);
            runtime.stale_reported = false;
        }
    }

    if runtime.last_smoothing_log.elapsed() >= Duration::from_secs(5) {
        let stats = runtime.smoother.stats();
        info!(
            target: "fly_ruler_proto_msfs.bridge",
            aircraft_id,
            object_id = runtime.session.object_id,
            buffer_depth = runtime.smoother.len(),
            accepted_samples = stats.accepted_samples - runtime.last_smoothing_stats.accepted_samples,
            dropped_out_of_order = stats.dropped_out_of_order - runtime.last_smoothing_stats.dropped_out_of_order,
            dropped_invalid_timestamp = stats.dropped_invalid_timestamp - runtime.last_smoothing_stats.dropped_invalid_timestamp,
            latest_frames = stats.latest_frames - runtime.last_smoothing_stats.latest_frames,
            interpolated_frames = stats.interpolated_frames - runtime.last_smoothing_stats.interpolated_frames,
            extrapolated_frames = stats.extrapolated_frames - runtime.last_smoothing_stats.extrapolated_frames,
            held_frames = stats.held_frames - runtime.last_smoothing_stats.held_frames,
            "MSFS live smoothing statistics"
        );
        runtime.last_smoothing_stats = stats;
        runtime.last_smoothing_log = Instant::now();
    }
    Ok(())
}

pub(crate) fn resolve_sample(
    store: &TimeSeriesStore,
    playback_state: &PlaybackSnapshot,
    aircraft_id: &str,
) -> Option<TimestampedState> {
    let id = aircraft_id.to_owned();
    match playback_state.mode {
        PlaybackMode::Live => store
            .is_spawned_at(&id, f64::INFINITY)
            .then(|| store.get_latest(&id))
            .flatten(),
        PlaybackMode::ReplayPaused | PlaybackMode::ReplayPlaying => {
            let cursor = playback_state.cursor_secs?;
            store
                .is_spawned_at(&id, cursor)
                .then(|| store.get_state_at_or_before(&id, cursor))
                .flatten()
        }
    }
}

fn apply_state(
    simulator: &mut SimConnectClient,
    aircraft_id: &str,
    runtime: &mut AircraftRuntime,
    state: &AircraftState,
    source: Option<FrameSource>,
    live_stale: bool,
) -> Result<(), SimConnectError> {
    let warnings = optional_field_warnings(state);
    if warnings != runtime.last_optional_warnings {
        for warning in &warnings {
            warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, warning, "invalid optional aircraft field");
        }
        runtime.last_optional_warnings = warnings;
    }
    match frame_from_state(state) {
        Ok(frame) => {
            runtime.session.apply(simulator, frame)?;
            if source == Some(FrameSource::Held) && live_stale && !runtime.stale_reported {
                warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, "aircraft state is stale; holding final MSFS pose");
                runtime.stale_reported = true;
            }
        }
        Err(error) => {
            warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id, "ignoring invalid aircraft state");
        }
    }
    Ok(())
}
