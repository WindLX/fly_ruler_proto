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
mod simconnect;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use fly_ruler_proto_core::{init_logging, KernelRuntime, PlaybackMode, TimeSeriesStore};
    use fly_ruler_proto_msfs::{
        frame_from_state, optional_field_warnings, select_aircraft_at, BridgeSession,
    };
    use simconnect::{SimConnectClient, SimConnectError};
    use tracing::{error, info, warn};

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

    let tick = Duration::from_secs_f64(1.0 / settings.tick_hz);
    let stale_timeout = Duration::from_millis(settings.stale_timeout_ms);

    while running.load(Ordering::SeqCst) {
        let simulator = match SimConnectClient::connect() {
            Ok(client) => client,
            Err(error) => {
                warn!(target: "fly_ruler_proto_msfs.bridge", %error, "waiting for MSFS 2024 SimConnect");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        info!(target: "fly_ruler_proto_msfs.bridge", "SimConnect connected; waiting for a valid FlyRuler aircraft state");
        let mut session = BridgeSession::new(simulator);
        let mut selected: Option<String> = None;
        let mut last_sample_key: Option<(u64, u64)> = None;
        let mut last_state = None;
        let mut last_new_sample: Option<Instant> = None;
        let mut stale_reported = false;
        let mut last_optional_warnings = Vec::new();
        let mut reconnect = false;

        while running.load(Ordering::SeqCst) {
            let loop_start = Instant::now();
            if let Err(error) = session.simulator_mut().pump() {
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
                    if let Err(error) = session.release() {
                        error!(target: "fly_ruler_proto_msfs.bridge", %error, "failed to release MSFS motion");
                    }
                    selected = None;
                    last_sample_key = None;
                    last_state = None;
                    last_new_sample = None;
                    stale_reported = false;
                    last_optional_warnings.clear();
                }
            }

            if selected.is_none() {
                selected =
                    select_aircraft_at(&store, settings.aircraft_id.as_deref(), selection_cursor);
                if let Some(id) = &selected {
                    info!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, "selected FlyRuler aircraft");
                }
            }

            if let Some(id) = selected.as_ref() {
                if let Some(resolved) = playback.resolve_aircraft_with(&playback_state, id) {
                    let sample = resolved.sample;
                    let sample_key = (sample.timestamp_secs.to_bits(), playback_state.revision);
                    if last_sample_key != Some(sample_key)
                        || last_state.as_ref() != Some(&sample.state)
                    {
                        let warnings = optional_field_warnings(&sample.state);
                        if warnings != last_optional_warnings {
                            for warning in &warnings {
                                warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, warning, "invalid optional aircraft field");
                            }
                            last_optional_warnings = warnings;
                        }
                        match frame_from_state(&sample.state) {
                            Ok(frame) => {
                                if let Err(error) = session.apply(frame) {
                                    error!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, "failed to write MSFS state");
                                    reconnect = true;
                                    break;
                                }
                                last_sample_key = Some(sample_key);
                                last_state = Some(sample.state.clone());
                                last_new_sample = Some(Instant::now());
                                stale_reported = false;
                            }
                            Err(error) => {
                                warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, "ignoring invalid aircraft state");
                                last_sample_key = Some(sample_key);
                                last_state = Some(sample.state.clone());
                            }
                        }
                    } else if !stale_reported
                        && playback_state.mode == PlaybackMode::Live
                        && last_new_sample.is_some_and(|seen| seen.elapsed() >= stale_timeout)
                    {
                        warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, "aircraft state is stale; holding final MSFS pose");
                        stale_reported = true;
                    }
                }
            }

            thread::sleep(tick.saturating_sub(loop_start.elapsed()));
        }

        if let Err(error) = session.release() {
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
