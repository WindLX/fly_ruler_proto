mod config;

#[cfg(not(windows))]
fn main() {
    let settings = config::load().ok();
    let logging = settings
        .as_ref()
        .map(|settings| &settings.runtime.logging)
        .cloned()
        .unwrap_or_default();
    fly_ruler_proto_core::init_logging(&logging);
    tracing::error!(
        target: "fly_ruler_proto_msfs.bridge",
        "fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton"
    );
    std::process::exit(2);
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use fly_ruler_proto_core::{
        init_logging, KernelRuntime, PlaybackMode, PlaybackSnapshot, TimeSeriesStore,
        TimestampedState,
    };
    use fly_ruler_proto_msfs::simconnect::{
        MsfsObjectId, SimConnectClient, SimConnectError, OBJECT_USER,
    };
    use fly_ruler_proto_msfs::smoothing::{
        FrameSource, LiveFrameBuffer, PushResult, SmoothingMode, SmoothingStats,
    };
    use fly_ruler_proto_msfs::{
        frame_control_values, frame_from_state, optional_field_warnings, select_aircraft_at,
        GearEventTracker, MsfsFrame,
    };
    use tracing::{error, info, warn};

    struct ObjectSession {
        object_id: MsfsObjectId,
        frozen: bool,
        user_aircraft: bool,
    }

    impl ObjectSession {
        fn user() -> Self {
            Self {
                object_id: OBJECT_USER,
                frozen: false,
                user_aircraft: true,
            }
        }

        fn ai(object_id: MsfsObjectId) -> Self {
            Self {
                object_id,
                frozen: false,
                user_aircraft: false,
            }
        }

        fn apply(
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
            for (offset, ratio) in frame.engines.ratios.into_iter().enumerate() {
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
            command: fly_ruler_proto_msfs::GearCommand,
        ) -> Result<(), SimConnectError> {
            simulator.set_landing_gear_for_object(self.object_id, command)
        }

        fn release(&mut self, simulator: &mut SimConnectClient) -> Result<(), SimConnectError> {
            if self.user_aircraft && self.frozen {
                simulator.set_frozen_for_object(self.object_id, false)?;
                self.frozen = false;
            }
            Ok(())
        }
    }

    struct AircraftRuntime {
        session: ObjectSession,
        smoother: LiveFrameBuffer,
        last_sample_key: Option<(u64, u64)>,
        last_state: Option<fly_ruler_proto_core::pb::AircraftState>,
        last_new_sample: Option<Instant>,
        stale_reported: bool,
        last_optional_warnings: Vec<&'static str>,
        gear_events: GearEventTracker,
        last_playback_mode: Option<PlaybackMode>,
        last_smoothing_stats: SmoothingStats,
        last_smoothing_log: Instant,
    }

    impl AircraftRuntime {
        fn new(
            session: ObjectSession,
            smoothing: fly_ruler_proto_msfs::smoothing::LiveSmoothingConfig,
        ) -> Self {
            Self {
                session,
                smoother: LiveFrameBuffer::new(smoothing),
                last_sample_key: None,
                last_state: None,
                last_new_sample: None,
                stale_reported: false,
                last_optional_warnings: Vec::new(),
                gear_events: GearEventTracker::default(),
                last_playback_mode: None,
                last_smoothing_stats: SmoothingStats::default(),
                last_smoothing_log: Instant::now(),
            }
        }

        fn reset_timeline_state(&mut self) {
            self.smoother.reset();
            self.last_sample_key = None;
            self.last_state = None;
            self.last_new_sample = None;
            self.stale_reported = false;
            self.last_optional_warnings.clear();
            self.gear_events.reset();
            self.last_smoothing_stats = SmoothingStats::default();
            self.last_smoothing_log = Instant::now();
        }
    }

    struct PendingAi {
        request_id: u32,
        send_id: u32,
        first_frame: MsfsFrame,
        requested_at: Instant,
    }

    struct AiRegistry {
        active: HashMap<String, AircraftRuntime>,
        pending: HashMap<String, PendingAi>,
        retry_after: HashMap<String, Instant>,
    }

    impl AiRegistry {
        fn new() -> Self {
            Self {
                active: HashMap::new(),
                pending: HashMap::new(),
                retry_after: HashMap::new(),
            }
        }

        fn clear(&mut self, simulator: &mut SimConnectClient) {
            for (id, runtime) in self.active.drain() {
                if let Err(error) = simulator.remove_ai_object(runtime.session.object_id) {
                    warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, object_id = runtime.session.object_id, "failed to remove MSFS AI aircraft");
                }
            }
            self.pending.clear();
            self.retry_after.clear();
        }

        fn remove_missing(&mut self, simulator: &mut SimConnectClient, keep: &HashSet<String>) {
            self.pending.retain(|id, _| keep.contains(id));
            let remove: Vec<_> = self
                .active
                .keys()
                .filter(|id| !keep.contains(*id))
                .cloned()
                .collect();
            for id in remove {
                if let Some(runtime) = self.active.remove(&id) {
                    if let Err(error) = simulator.remove_ai_object(runtime.session.object_id) {
                        warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, object_id = runtime.session.object_id, "failed to remove despawned MSFS AI aircraft");
                    } else {
                        info!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, object_id = runtime.session.object_id, "removed MSFS AI aircraft");
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_aircraft_runtime(
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
            let sample_key = (sample.timestamp_secs.to_bits(), playback_state.revision);
            let raw_changed = runtime.last_sample_key != Some(sample_key)
                || runtime.last_state.as_ref() != Some(&sample.state);
            if raw_changed {
                match runtime
                    .smoother
                    .push(sample.timestamp_secs, sample.state.clone(), loop_start)
                {
                    PushResult::Accepted | PushResult::UpdatedDuplicate => {
                        runtime.last_new_sample = Some(loop_start);
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
                        stale_timeout,
                    )?;
                }
            } else if !runtime.stale_reported
                && runtime
                    .last_new_sample
                    .is_some_and(|seen| seen.elapsed() >= stale_timeout)
            {
                warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id, "aircraft state is stale; holding final MSFS pose");
                runtime.stale_reported = true;
            }
        } else {
            let sample_key = (sample.timestamp_secs.to_bits(), playback_state.revision);
            if runtime.last_sample_key != Some(sample_key)
                || runtime.last_state.as_ref() != Some(&sample.state)
            {
                match apply_state(
                    simulator,
                    aircraft_id,
                    runtime,
                    &sample.state,
                    None,
                    stale_timeout,
                ) {
                    Ok(()) => {
                        runtime.last_sample_key = Some(sample_key);
                        runtime.last_state = Some(sample.state);
                        runtime.stale_reported = false;
                    }
                    Err(error) => return Err(error),
                }
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

    fn resolve_sample(
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
        state: &fly_ruler_proto_core::pb::AircraftState,
        source: Option<FrameSource>,
        stale_timeout: Duration,
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
                if source == Some(FrameSource::Held)
                    && !runtime.stale_reported
                    && runtime
                        .last_new_sample
                        .is_some_and(|seen| seen.elapsed() >= stale_timeout)
                {
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

    #[allow(clippy::too_many_arguments)]
    fn update_ai_registry(
        simulator: &mut SimConnectClient,
        store: &TimeSeriesStore,
        playback_state: &PlaybackSnapshot,
        user_aircraft_id: Option<&str>,
        registry: &mut AiRegistry,
        aircraft_title: &str,
        max_ai_aircraft: usize,
        smoothing: &fly_ruler_proto_msfs::smoothing::LiveSmoothingConfig,
        loop_start: Instant,
        stale_timeout: Duration,
        smoothing_mode: SmoothingMode,
    ) -> Result<(), SimConnectError> {
        let candidates = ai_candidates(store, playback_state, user_aircraft_id, max_ai_aircraft);
        let keep: HashSet<_> = candidates.iter().cloned().collect();
        registry.remove_missing(simulator, &keep);

        for exception in simulator.drain_exceptions() {
            let failed_id = registry.pending.iter().find_map(|(id, pending)| {
                (pending.send_id == exception.send_id).then(|| id.clone())
            });
            if let Some(id) = failed_id {
                let Some(pending) = registry.pending.remove(&id) else {
                    continue;
                };
                warn!(
                    target: "fly_ruler_proto_msfs.bridge",
                    aircraft_id = id,
                    request_id = pending.request_id,
                    send_id = pending.send_id,
                    exception = exception.exception,
                    exception_name = exception.name(),
                    index = exception.index,
                    aircraft_title,
                    "MSFS rejected AI aircraft creation; check that ai_aircraft_title is an installed preset/container title"
                );
                registry
                    .retry_after
                    .insert(id, Instant::now() + Duration::from_secs(5));
            } else {
                warn!(
                    target: "fly_ruler_proto_msfs.bridge",
                    exception = exception.exception,
                    exception_name = exception.name(),
                    send_id = exception.send_id,
                    index = exception.index,
                    "SimConnect reported non-fatal asynchronous exception"
                );
            }
        }

        let timed_out: Vec<_> = registry
            .pending
            .iter()
            .filter(|(_, pending)| pending.requested_at.elapsed() >= Duration::from_secs(10))
            .map(|(id, _)| id.clone())
            .collect();
        for id in timed_out {
            if let Some(pending) = registry.pending.remove(&id) {
                warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, request_id = pending.request_id, "timed out waiting for MSFS AI aircraft assignment");
                registry
                    .retry_after
                    .insert(id, Instant::now() + Duration::from_secs(5));
            }
        }

        let pending_ids: Vec<_> = registry.pending.keys().cloned().collect();
        for id in pending_ids {
            let Some(pending) = registry.pending.get(&id) else {
                continue;
            };
            let Some(object_id) = simulator.take_assigned_object(pending.request_id) else {
                continue;
            };
            let Some(pending) = registry.pending.remove(&id) else {
                continue;
            };
            if !keep.contains(&id) {
                simulator.remove_ai_object(object_id)?;
                continue;
            }
            simulator.release_ai_control(object_id)?;
            let mut runtime = AircraftRuntime::new(ObjectSession::ai(object_id), smoothing.clone());
            runtime.session.apply(simulator, pending.first_frame)?;
            info!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, object_id, "MSFS AI aircraft assigned and frozen");
            registry.active.insert(id, runtime);
        }

        let now = Instant::now();
        for id in candidates.iter() {
            if registry.active.contains_key(id) || registry.pending.contains_key(id) {
                continue;
            }
            if registry.active.len() + registry.pending.len() >= max_ai_aircraft {
                break;
            }
            if registry
                .retry_after
                .get(id)
                .is_some_and(|retry_after| *retry_after > now)
            {
                continue;
            }
            let Some(sample) = resolve_sample(store, playback_state, id) else {
                continue;
            };
            let frame = match frame_from_state(&sample.state) {
                Ok(frame) => frame,
                Err(error) => {
                    warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, "cannot create AI aircraft from invalid initial state");
                    continue;
                }
            };
            let airspeed_knots = frame.airdata.map(|airdata| {
                (airdata.velocity_body_x_mps.powi(2)
                    + airdata.velocity_body_y_mps.powi(2)
                    + airdata.velocity_body_z_mps.powi(2))
                .sqrt()
                    * 1.943_844_492
            });
            let tail_number = ai_tail_number(id);
            match simulator.create_ai_aircraft(
                aircraft_title,
                &tail_number,
                frame.pose,
                airspeed_knots,
            ) {
                Ok(request) => {
                    registry.pending.insert(
                        id.clone(),
                        PendingAi {
                            request_id: request.request_id,
                            send_id: request.send_id,
                            first_frame: frame,
                            requested_at: loop_start,
                        },
                    );
                    info!(
                        target: "fly_ruler_proto_msfs.bridge",
                        aircraft_id = id,
                        request_id = request.request_id,
                        send_id = request.send_id,
                        aircraft_title,
                        tail_number,
                        "requested MSFS AI aircraft"
                    );
                }
                Err(error) => {
                    warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, aircraft_title, "failed to request MSFS AI aircraft");
                    registry
                        .retry_after
                        .insert(id.clone(), now + Duration::from_secs(5));
                }
            }
        }

        for id in candidates {
            if let Some(runtime) = registry.active.get_mut(&id) {
                process_aircraft_runtime(
                    simulator,
                    store,
                    playback_state,
                    &id,
                    runtime,
                    loop_start,
                    stale_timeout,
                    smoothing_mode,
                )?;
            }
        }
        Ok(())
    }

    fn ai_candidates(
        store: &TimeSeriesStore,
        playback_state: &PlaybackSnapshot,
        user_aircraft_id: Option<&str>,
        max_ai_aircraft: usize,
    ) -> Vec<String> {
        let cursor = match playback_state.mode {
            PlaybackMode::Live => f64::INFINITY,
            PlaybackMode::ReplayPaused | PlaybackMode::ReplayPlaying => {
                playback_state.cursor_secs.unwrap_or(f64::NEG_INFINITY)
            }
        };
        let mut ids: Vec<_> = store
            .get_aircraft_ids()
            .into_iter()
            .filter(|id| user_aircraft_id != Some(id.as_str()))
            .filter(|id| store.is_spawned_at(id, cursor))
            .collect();
        ids.sort();
        ids.truncate(max_ai_aircraft);
        ids
    }

    fn ai_tail_number(aircraft_id: &str) -> String {
        let suffix: String = aircraft_id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(8)
            .collect();
        format!("FR{suffix}").chars().take(12).collect()
    }

    let settings = config::load()?;
    init_logging(&settings.runtime.logging);
    info!(
        target: "fly_ruler_proto_msfs.bridge",
        config_path = ?settings.config_path,
        "MSFS bridge configuration loaded"
    );

    let running = Arc::new(AtomicBool::new(true));
    let signal_running = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_running.store(false, Ordering::SeqCst);
    })?;

    let async_runtime = tokio::runtime::Runtime::new()?;
    let store = Arc::new(TimeSeriesStore::new());
    let mut kernel = KernelRuntime::with_config(Arc::clone(&store), settings.runtime.clone());
    async_runtime.block_on(kernel.start_server(&settings.listen))?;
    info!(
        target: "fly_ruler_proto_msfs.bridge",
        addr = %kernel.udp_local_addr()?,
        "FlyRuler UDP server started"
    );
    if settings.management_enabled {
        async_runtime.block_on(kernel.start_management_server(&settings.http_listen))?;
        info!(
            target: "fly_ruler_proto_msfs.bridge",
            addr = %kernel.management_local_addr()?,
            "FlyRuler HTTP/WebSocket management server started"
        );
    }
    let playback = kernel.playback();

    let tick = Duration::from_secs_f64(1.0 / settings.render_hz);
    let stale_timeout = Duration::from_millis(settings.stale_timeout_ms);
    info!(
        target: "fly_ruler_proto_msfs.bridge",
        tick_hz = settings.tick_hz,
        render_hz = settings.render_hz,
        enable_ai_aircraft = settings.enable_ai_aircraft,
        ai_aircraft_title = settings.ai_aircraft_title,
        max_ai_aircraft = settings.max_ai_aircraft,
        smoothing_mode = settings.smoothing.mode.as_str(),
        interpolation_delay_ms = settings.smoothing.interpolation_delay.as_millis(),
        max_extrapolation_ms = settings.smoothing.max_extrapolation.as_millis(),
        "MSFS bridge live smoothing configured"
    );

    while running.load(Ordering::SeqCst) {
        let mut simulator = match SimConnectClient::connect() {
            Ok(client) => client,
            Err(error) => {
                warn!(target: "fly_ruler_proto_msfs.bridge", %error, "waiting for MSFS 2024 SimConnect");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        info!(target: "fly_ruler_proto_msfs.bridge", "SimConnect connected; waiting for a valid FlyRuler aircraft state");
        let mut user_runtime =
            AircraftRuntime::new(ObjectSession::user(), settings.smoothing.clone());
        let mut ai_registry = AiRegistry::new();
        let mut selected: Option<String> = None;
        let mut reconnect = false;

        while running.load(Ordering::SeqCst) {
            let loop_start = Instant::now();
            if let Err(error) = simulator.pump() {
                match error {
                    SimConnectError::Disconnected => {
                        warn!(target: "fly_ruler_proto_msfs.bridge", "MSFS disconnected; waiting to reconnect");
                    }
                    _ => {
                        error!(target: "fly_ruler_proto_msfs.bridge", %error, "SimConnect pump failed")
                    }
                }
                reconnect = true;
                break;
            }

            let playback_state = playback.snapshot();
            let selection_cursor = playback_state.cursor_secs.unwrap_or(f64::NEG_INFINITY);

            if let Some(id) = selected.as_ref() {
                let spawned = playback
                    .resolve_aircraft_with(&playback_state, id)
                    .is_some_and(|resolved| resolved.spawned);
                if !spawned {
                    info!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, "aircraft despawned; releasing MSFS motion");
                    if let Err(error) = user_runtime.session.release(&mut simulator) {
                        error!(target: "fly_ruler_proto_msfs.bridge", %error, "failed to release MSFS motion");
                    }
                    selected = None;
                    user_runtime.reset_timeline_state();
                }
            }

            if selected.is_none() {
                selected =
                    select_aircraft_at(&store, settings.aircraft_id.as_deref(), selection_cursor);
                if let Some(id) = &selected {
                    info!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, "selected FlyRuler aircraft");
                    user_runtime.reset_timeline_state();
                }
            }

            if let Some(id) = selected.as_ref() {
                if let Err(error) = process_aircraft_runtime(
                    &mut simulator,
                    &store,
                    &playback_state,
                    id,
                    &mut user_runtime,
                    loop_start,
                    stale_timeout,
                    settings.smoothing.mode,
                ) {
                    error!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, "failed to update MSFS user aircraft");
                    reconnect = true;
                    break;
                }
            }

            if settings.enable_ai_aircraft {
                if let Err(error) = update_ai_registry(
                    &mut simulator,
                    &store,
                    &playback_state,
                    selected.as_deref(),
                    &mut ai_registry,
                    &settings.ai_aircraft_title,
                    settings.max_ai_aircraft,
                    &settings.smoothing,
                    loop_start,
                    stale_timeout,
                    settings.smoothing.mode,
                ) {
                    error!(target: "fly_ruler_proto_msfs.bridge", %error, "failed to update MSFS AI aircraft");
                    reconnect = true;
                    break;
                }
            } else {
                ai_registry.clear(&mut simulator);
            }

            thread::sleep(tick.saturating_sub(loop_start.elapsed()));
        }

        ai_registry.clear(&mut simulator);
        if let Err(error) = user_runtime.session.release(&mut simulator) {
            error!(target: "fly_ruler_proto_msfs.bridge", %error, "failed to restore MSFS freeze state");
        }
        if reconnect && running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
        }
    }

    async_runtime.block_on(kernel.stop_management_server());
    async_runtime.block_on(kernel.stop_server());
    info!(target: "fly_ruler_proto_msfs.bridge", "FlyRuler MSFS bridge stopped");
    Ok(())
}
