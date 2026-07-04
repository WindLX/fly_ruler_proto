use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "fly-ruler-msfs-bridge",
    about = "Drive the MSFS 2024 user aircraft from FlyRuler UDP state"
)]
struct Args {
    /// FlyRuler UDP server listen address.
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,
    /// Optional lowercase hexadecimal FlyRuler aircraft UUID.
    #[arg(long)]
    aircraft_id: Option<String>,
    /// Bridge polling frequency.
    #[arg(long, default_value_t = 240.0)]
    tick_hz: f64,
    /// Warn and hold the final pose after this state timeout.
    #[arg(long, default_value_t = 500)]
    stale_timeout_ms: u64,
    /// HTTP/WebSocket management listen address.
    #[arg(long, default_value = "127.0.0.1:8081")]
    http_listen: String,
    /// Root directory for named persisted sessions.
    #[arg(long, default_value = "./sessions")]
    data_root: PathBuf,
    /// WebSocket aggregate snapshot frequency.
    #[arg(long, default_value_t = 30.0)]
    ws_hz: f64,
    /// Additional browser origin allowed to call the management API.
    #[arg(long = "cors-origin")]
    cors_origins: Vec<String>,
    /// Disable the embedded HTTP/WebSocket management service.
    #[arg(long)]
    no_http: bool,
}

#[cfg(not(windows))]
fn main() {
    let _ = Args::parse();
    eprintln!(
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

    use fly_ruler_proto_core::{
        KernelRuntime, ManagementConfig, PlaybackMode, RuntimeConfig, TimeSeriesStore,
    };
    use fly_ruler_proto_msfs::{
        frame_from_state, optional_field_warnings, select_aircraft_at, BridgeSession,
    };
    use simconnect::{SimConnectClient, SimConnectError};

    let args = Args::parse();
    if !args.tick_hz.is_finite() || args.tick_hz <= 0.0 {
        return Err("--tick-hz must be finite and greater than zero".into());
    }
    if let Some(id) = &args.aircraft_id {
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("--aircraft-id must be a 32-character hexadecimal UUID".into());
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let signal_running = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_running.store(false, Ordering::SeqCst);
    })?;

    let async_runtime = tokio::runtime::Runtime::new()?;
    let store = Arc::new(TimeSeriesStore::new());
    let default_management = ManagementConfig::default();
    let config = RuntimeConfig {
        management: ManagementConfig {
            data_root: args.data_root.clone(),
            websocket_hz: args.ws_hz,
            cors_origins: if args.cors_origins.is_empty() {
                default_management.cors_origins
            } else {
                args.cors_origins.clone()
            },
        },
        ..RuntimeConfig::default()
    };
    let mut kernel = KernelRuntime::with_config(Arc::clone(&store), config);
    async_runtime.block_on(kernel.start_server(&args.listen))?;
    println!("FlyRuler UDP listening on {}", kernel.udp_local_addr()?);
    if !args.no_http {
        async_runtime.block_on(kernel.start_management_server(&args.http_listen))?;
        println!(
            "FlyRuler HTTP/WebSocket listening on {}",
            kernel.management_local_addr()?
        );
    }
    let playback = kernel.playback();

    let tick = Duration::from_secs_f64(1.0 / args.tick_hz);
    let stale_timeout = Duration::from_millis(args.stale_timeout_ms);

    while running.load(Ordering::SeqCst) {
        let simulator = match SimConnectClient::connect() {
            Ok(client) => client,
            Err(error) => {
                eprintln!("waiting for MSFS 2024 SimConnect: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        println!("SimConnect connected; waiting for a valid FlyRuler aircraft state");
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
                        eprintln!("MSFS disconnected; waiting to reconnect");
                    }
                    _ => eprintln!("SimConnect error: {error}"),
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
                    println!("FlyRuler aircraft {id} despawned; releasing MSFS motion");
                    if let Err(error) = session.release() {
                        eprintln!("failed to release MSFS motion: {error}");
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
                    select_aircraft_at(&store, args.aircraft_id.as_deref(), selection_cursor);
                if let Some(id) = &selected {
                    println!("selected FlyRuler aircraft {id}");
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
                                eprintln!("warning: aircraft {id}: {warning}");
                            }
                            last_optional_warnings = warnings;
                        }
                        match frame_from_state(&sample.state) {
                            Ok(frame) => {
                                if let Err(error) = session.apply(frame) {
                                    eprintln!("failed to write MSFS state: {error}");
                                    reconnect = true;
                                    break;
                                }
                                last_sample_key = Some(sample_key);
                                last_state = Some(sample.state.clone());
                                last_new_sample = Some(Instant::now());
                                stale_reported = false;
                            }
                            Err(error) => {
                                eprintln!("ignoring invalid state for aircraft {id}: {error}");
                                last_sample_key = Some(sample_key);
                                last_state = Some(sample.state.clone());
                            }
                        }
                    } else if !stale_reported
                        && playback_state.mode == PlaybackMode::Live
                        && last_new_sample.is_some_and(|seen| seen.elapsed() >= stale_timeout)
                    {
                        eprintln!(
                            "warning: aircraft {id} state is stale; holding the final MSFS pose"
                        );
                        stale_reported = true;
                    }
                }
            }

            thread::sleep(tick.saturating_sub(loop_start.elapsed()));
        }

        if let Err(error) = session.release() {
            eprintln!("failed to restore MSFS freeze state: {error}");
        }
        if reconnect && running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
        }
    }

    async_runtime.block_on(kernel.stop_management_server());
    async_runtime.block_on(kernel.stop_server());
    println!("FlyRuler MSFS bridge stopped");
    Ok(())
}
