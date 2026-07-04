use clap::Parser;

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

    use fly_ruler_proto_core::{KernelRuntime, TimeSeriesStore};
    use fly_ruler_proto_msfs::{
        frame_from_state, is_spawned, optional_field_warnings, select_aircraft, BridgeSession,
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
    let mut kernel = KernelRuntime::new(Arc::clone(&store));
    async_runtime.block_on(kernel.start_server(&args.listen))?;
    println!("FlyRuler UDP listening on {}", kernel.udp_local_addr()?);

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
        let mut last_timestamp_bits: Option<u64> = None;
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

            if let Some(id) = selected.as_ref() {
                if !is_spawned(&store, id) {
                    println!("FlyRuler aircraft {id} despawned; releasing MSFS motion");
                    if let Err(error) = session.release() {
                        eprintln!("failed to release MSFS motion: {error}");
                    }
                    selected = None;
                    last_timestamp_bits = None;
                    last_new_sample = None;
                    stale_reported = false;
                    last_optional_warnings.clear();
                }
            }

            if selected.is_none() {
                selected = select_aircraft(&store, args.aircraft_id.as_deref());
                if let Some(id) = &selected {
                    println!("selected FlyRuler aircraft {id}");
                }
            }

            if let Some(id) = selected.as_ref() {
                if let Some(sample) = store.get_latest(id) {
                    let timestamp_bits = sample.timestamp_secs.to_bits();
                    if last_timestamp_bits != Some(timestamp_bits) {
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
                                last_timestamp_bits = Some(timestamp_bits);
                                last_new_sample = Some(Instant::now());
                                stale_reported = false;
                            }
                            Err(error) => {
                                eprintln!("ignoring invalid state for aircraft {id}: {error}");
                                last_timestamp_bits = Some(timestamp_bits);
                            }
                        }
                    } else if !stale_reported
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

    async_runtime.block_on(kernel.stop_server());
    println!("FlyRuler MSFS bridge stopped");
    Ok(())
}
