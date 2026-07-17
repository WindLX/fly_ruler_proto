use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fly_ruler_proto_core::{init_logging, KernelRuntime, TimeSeriesStore};
use fly_ruler_proto_msfs::select_aircraft_at;
use fly_ruler_proto_msfs::simconnect::{SimConnectClient, SimConnectError};
use tracing::{error, info, warn};

use crate::ai::AiRegistry;
use crate::aircraft::{process, AircraftRuntime, ObjectSession};
use crate::config::BridgeConfig;

pub(crate) fn run(settings: BridgeConfig) -> Result<(), Box<dyn std::error::Error>> {
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
                if let Err(error) = process(
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
                if let Err(error) = ai_registry.update(
                    &mut simulator,
                    &store,
                    &playback_state,
                    selected.as_deref(),
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
