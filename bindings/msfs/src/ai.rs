use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fly_ruler_proto_core::{PlaybackMode, PlaybackSnapshot, TimeSeriesStore};
use fly_ruler_proto_msfs::simconnect::{SimConnectClient, SimConnectError};
use fly_ruler_proto_msfs::smoothing::{LiveSmoothingConfig, SmoothingMode};
use fly_ruler_proto_msfs::{frame_from_state, MsfsFrame};
use tracing::{info, warn};

use crate::aircraft::{process, resolve_sample, AircraftRuntime, ObjectSession};

struct PendingAi {
    request_id: u32,
    send_id: u32,
    first_frame: MsfsFrame,
    requested_at: Instant,
}

pub(crate) struct AiRegistry {
    active: HashMap<String, AircraftRuntime>,
    pending: HashMap<String, PendingAi>,
    retry_after: HashMap<String, Instant>,
}

impl AiRegistry {
    pub(crate) fn new() -> Self {
        Self {
            active: HashMap::new(),
            pending: HashMap::new(),
            retry_after: HashMap::new(),
        }
    }

    pub(crate) fn clear(&mut self, simulator: &mut SimConnectClient) {
        for (id, runtime) in self.active.drain() {
            if let Err(error) = simulator.remove_ai_object(runtime.session.object_id) {
                warn!(target: "fly_ruler_proto_msfs.bridge", %error, aircraft_id = id, object_id = runtime.session.object_id, "failed to remove MSFS AI aircraft");
            }
        }
        self.pending.clear();
        self.retry_after.clear();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        simulator: &mut SimConnectClient,
        store: &TimeSeriesStore,
        playback_state: &PlaybackSnapshot,
        user_aircraft_id: Option<&str>,
        aircraft_title: &str,
        max_ai_aircraft: usize,
        smoothing: &LiveSmoothingConfig,
        loop_start: Instant,
        stale_timeout: Duration,
        smoothing_mode: SmoothingMode,
    ) -> Result<(), SimConnectError> {
        let candidates = candidates(store, playback_state, user_aircraft_id, max_ai_aircraft);
        let keep: HashSet<_> = candidates.iter().cloned().collect();
        self.remove_missing(simulator, &keep);

        for exception in simulator.drain_exceptions() {
            let failed_id = self.pending.iter().find_map(|(id, pending)| {
                (pending.send_id == exception.send_id).then(|| id.clone())
            });
            if let Some(id) = failed_id {
                let Some(pending) = self.pending.remove(&id) else {
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
                self.retry_after
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

        let timed_out: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.requested_at.elapsed() >= Duration::from_secs(10))
            .map(|(id, _)| id.clone())
            .collect();
        for id in timed_out {
            if let Some(pending) = self.pending.remove(&id) {
                warn!(target: "fly_ruler_proto_msfs.bridge", aircraft_id = id, request_id = pending.request_id, "timed out waiting for MSFS AI aircraft assignment");
                self.retry_after
                    .insert(id, Instant::now() + Duration::from_secs(5));
            }
        }

        let pending_ids: Vec<_> = self.pending.keys().cloned().collect();
        for id in pending_ids {
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            let Some(object_id) = simulator.take_assigned_object(pending.request_id) else {
                continue;
            };
            let Some(pending) = self.pending.remove(&id) else {
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
            self.active.insert(id, runtime);
        }

        let now = Instant::now();
        for id in &candidates {
            if self.active.contains_key(id) || self.pending.contains_key(id) {
                continue;
            }
            if self.active.len() + self.pending.len() >= max_ai_aircraft {
                break;
            }
            if self
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
            let tail_number = tail_number(id);
            match simulator.create_ai_aircraft(
                aircraft_title,
                &tail_number,
                frame.pose,
                airspeed_knots,
            ) {
                Ok(request) => {
                    self.pending.insert(
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
                    self.retry_after
                        .insert(id.clone(), now + Duration::from_secs(5));
                }
            }
        }

        for id in candidates {
            if let Some(runtime) = self.active.get_mut(&id) {
                process(
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

fn candidates(
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

fn tail_number(aircraft_id: &str) -> String {
    let suffix: String = aircraft_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!("FR{suffix}").chars().take(12).collect()
}
