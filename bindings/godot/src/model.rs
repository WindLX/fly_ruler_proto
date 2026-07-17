use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fly_ruler_proto_core::{pb, Attitude};
use fly_ruler_proto_core::{
    LoggingFileConfig, ManagementFileConfig, PlaybackFileConfig, PlaybackMode, PlaybackSnapshot,
    TimeSeriesStore, TransportFileConfig, RUNTIME_CONFIG_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GodotRuntimeFileConfig {
    pub schema_version: u32,
    pub transport: TransportFileConfig,
    pub management: ManagementFileConfig,
    pub visualization: VisualizationFileConfig,
    pub playback: PlaybackFileConfig,
    pub logging: LoggingFileConfig,
}

impl Default for GodotRuntimeFileConfig {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_CONFIG_SCHEMA_VERSION,
            transport: TransportFileConfig::default(),
            management: ManagementFileConfig {
                enabled: true,
                listen: "127.0.0.1:18003".to_string(),
                data_root: "user://sessions".to_string(),
                web_root: "res://addons/fly_ruler_proto/web".to_string(),
                websocket_hz: 30.0,
            },
            visualization: VisualizationFileConfig::default(),
            playback: PlaybackFileConfig::default(),
            logging: LoggingFileConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisualizationFileConfig {
    pub snapshot_hz: f64,
    pub stale_timeout_secs: f64,
}

impl Default for VisualizationFileConfig {
    fn default() -> Self {
        Self {
            snapshot_hz: 60.0,
            stale_timeout_secs: 0.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    pub udp_listen: String,
    pub management_enabled: bool,
    pub management_listen: String,
    pub data_root: String,
    pub web_root: String,
    pub websocket_hz: f64,
    pub snapshot_hz: f64,
    pub heartbeat_interval_secs: u64,
    pub heartbeat_timeout_secs: u64,
    pub replay_min_speed: f64,
    pub replay_max_speed: f64,
    pub stale_timeout_secs: f64,
    pub log_level: String,
    pub log_file: Option<String>,
}

impl RuntimeSettings {
    pub fn validate(&self) -> Result<(), String> {
        validate_endpoint(&self.udp_listen, false, "udp_listen")?;
        if self.management_enabled {
            validate_endpoint(&self.management_listen, true, "management_listen")?;
        }
        for (name, value) in [
            ("websocket_hz", self.websocket_hz),
            ("snapshot_hz", self.snapshot_hz),
            ("replay_min_speed", self.replay_min_speed),
            ("replay_max_speed", self.replay_max_speed),
            ("stale_timeout_secs", self.stale_timeout_secs),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and greater than zero"));
            }
        }
        if self.replay_min_speed > self.replay_max_speed {
            return Err("replay_min_speed must not exceed replay_max_speed".to_string());
        }
        if self.heartbeat_interval_secs == 0
            || self.heartbeat_timeout_secs <= self.heartbeat_interval_secs
        {
            return Err(
                "heartbeat_timeout_secs must exceed a non-zero heartbeat_interval_secs".to_string(),
            );
        }
        if !matches!(
            self.log_level.as_str(),
            "trace" | "debug" | "info" | "warn" | "error"
        ) {
            return Err("log_level must be trace, debug, info, warn, or error".to_string());
        }
        Ok(())
    }

    pub fn validate_paths(&self) -> Result<(), String> {
        if self.data_root.trim().is_empty() {
            return Err("data_root must not be empty".to_string());
        }
        fs::create_dir_all(&self.data_root)
            .map_err(|error| format!("data_root is not writable: {error}"))?;
        if self.management_enabled {
            let web_root = Path::new(&self.web_root);
            if !web_root.join("index.html").is_file() {
                return Err("web_root must contain index.html".to_string());
            }
        }
        Ok(())
    }
}

fn validate_endpoint(value: &str, loopback_only: bool, name: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if let Ok(address) = trimmed.parse::<SocketAddr>() {
        if loopback_only && !address.ip().is_loopback() {
            return Err(format!("{name} must use a loopback address"));
        }
        return Ok(());
    }
    let Some((host, port)) = trimmed.rsplit_once(':') else {
        return Err(format!("{name} must be a valid host:port address"));
    };
    let valid_port = port.parse::<u16>().is_ok_and(|value| value > 0);
    let host = host.trim_matches(['[', ']']);
    let valid_host = host == "localhost" || host.parse::<IpAddr>().is_ok();
    if !valid_port || !valid_host {
        return Err(format!("{name} must be a valid host:port address"));
    }
    if loopback_only
        && host != "localhost"
        && !host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
    {
        return Err(format!("{name} must use a loopback address"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct AircraftSnapshotData {
    pub aircraft_id: String,
    pub name: String,
    pub toml_config: String,
    pub source_timestamp_secs: f64,
    pub stale: bool,
    pub state: pb::AircraftState,
}

#[derive(Debug, Clone)]
pub struct FrameSnapshotData {
    pub mode: PlaybackMode,
    pub cursor_secs: Option<f64>,
    pub speed: f64,
    pub bounds: Option<(f64, f64)>,
    pub revision: u64,
    pub generated_at_secs: f64,
    pub aircraft: Vec<AircraftSnapshotData>,
}

pub fn build_frame(
    store: &TimeSeriesStore,
    playback: &fly_ruler_proto_core::PlaybackController,
    snapshot: &PlaybackSnapshot,
    stale_timeout_secs: f64,
) -> FrameSnapshotData {
    let generated_at_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    let configs: HashMap<_, _> = store
        .aircraft_summaries()
        .into_iter()
        .map(|summary| (summary.id, summary.config))
        .collect();
    let mut aircraft = Vec::new();
    for aircraft_id in store.get_aircraft_ids() {
        let Some(resolved) = playback.resolve_aircraft_with(snapshot, &aircraft_id) else {
            continue;
        };
        if !resolved.spawned {
            continue;
        }
        if !state_is_renderable(&resolved.sample.state) {
            continue;
        }
        let config = configs.get(&aircraft_id).and_then(Option::as_ref);
        let stale_timeout = Duration::from_secs_f64(stale_timeout_secs);
        let stale = snapshot.mode == PlaybackMode::Live
            && store.live_state_is_stale(&aircraft_id, stale_timeout);
        aircraft.push(AircraftSnapshotData {
            aircraft_id,
            name: config.map_or_else(String::new, |value| value.name.clone()),
            toml_config: config.map_or_else(String::new, |value| value.toml_config.clone()),
            source_timestamp_secs: resolved.sample.timestamp_secs,
            stale,
            state: resolved.sample.state,
        });
    }
    aircraft.sort_by(|left, right| left.aircraft_id.cmp(&right.aircraft_id));
    FrameSnapshotData {
        mode: snapshot.mode,
        cursor_secs: snapshot.cursor_secs,
        speed: snapshot.speed,
        bounds: snapshot.bounds,
        revision: snapshot.revision,
        generated_at_secs,
        aircraft,
    }
}

fn state_is_renderable(state: &pb::AircraftState) -> bool {
    let Some(position) = &state.position else {
        return false;
    };
    let Some(attitude) = &state.attitude else {
        return false;
    };
    let finite_position = [position.x, position.y, position.z]
        .into_iter()
        .all(f64::is_finite);
    finite_position && Attitude::try_from(attitude).is_ok()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fly_ruler_proto_core::{Event, PlaybackController, ReplayConfig};

    use super::*;

    fn settings() -> RuntimeSettings {
        RuntimeSettings {
            udp_listen: "127.0.0.1:18002".to_string(),
            management_enabled: true,
            management_listen: "127.0.0.1:18003".to_string(),
            data_root: "sessions".to_string(),
            web_root: "web".to_string(),
            websocket_hz: 30.0,
            snapshot_hz: 60.0,
            heartbeat_interval_secs: 5,
            heartbeat_timeout_secs: 15,
            replay_min_speed: 0.1,
            replay_max_speed: 16.0,
            stale_timeout_secs: 0.5,
            log_level: "warn".to_string(),
            log_file: None,
        }
    }

    #[test]
    fn settings_validation_rejects_invalid_rates_and_heartbeat() {
        assert!(settings().validate().is_ok());
        let mut invalid = settings();
        invalid.snapshot_hz = 0.0;
        assert_eq!(
            invalid.validate().unwrap_err(),
            "snapshot_hz must be finite and greater than zero"
        );
        let mut invalid = settings();
        invalid.heartbeat_timeout_secs = 5;
        assert!(invalid
            .validate()
            .unwrap_err()
            .contains("heartbeat_timeout"));
        let mut invalid = settings();
        invalid.management_listen = "0.0.0.0:18003".to_string();
        assert!(invalid.validate().unwrap_err().contains("loopback"));
        let mut invalid = settings();
        invalid.log_level = "verbose".to_string();
        assert!(invalid.validate().unwrap_err().contains("log_level"));
    }

    #[test]
    fn file_defaults_keep_godot_virtual_paths_at_the_host_boundary() {
        let config = GodotRuntimeFileConfig::default();
        assert_eq!(config.management.data_root, "user://sessions");
        assert_eq!(
            config.management.web_root,
            "res://addons/fly_ruler_proto/web"
        );
    }

    #[test]
    fn frame_uses_one_cursor_and_filters_despawned_aircraft() {
        let store = Arc::new(TimeSeriesStore::new());
        let id = "11".repeat(16);
        store.append_event(
            id.clone(),
            1.0,
            Event::Spawn(Box::new(pb::AircraftSpawnInfo {
                name: "test".to_string(),
                toml_config: "model = 'test'".to_string(),
                initial_state: None,
                telemetry_schemas: Vec::new(),
            })),
        );
        store.append_state(
            id.clone(),
            2.0,
            pb::AircraftState {
                position: Some(pb::Vector3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }),
                attitude: Some(pb::Quaternion {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                ..pb::AircraftState::default()
            },
        );
        store.append_event(id, 3.0, Event::Despawn(pb::DespawnInfo { reason: None }));
        let playback = PlaybackController::new(Arc::clone(&store), ReplayConfig::default());
        let at_two = playback.seek(2.0).unwrap();
        let frame = build_frame(&store, &playback, &at_two, 0.5);
        assert_eq!(frame.revision, at_two.revision);
        assert_eq!(frame.aircraft.len(), 1);
        assert_eq!(frame.aircraft[0].name, "test");
        let at_three = playback.seek(3.0).unwrap();
        assert!(build_frame(&store, &playback, &at_three, 0.5)
            .aircraft
            .is_empty());
    }

    #[test]
    fn live_frame_freshness_uses_receipt_time_for_zero_based_simulation() {
        let store = Arc::new(TimeSeriesStore::new());
        let id = "11".repeat(16);
        let state = pb::AircraftState {
            position: Some(pb::Vector3 {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
            attitude: Some(pb::Quaternion {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            ..pb::AircraftState::default()
        };
        store.append_message(pb::Message {
            envelope: Some(pb::message::Envelope::Request(pb::Request {
                id: None,
                timestamp: 0.0,
                command: Some(pb::RequestCommand {
                    kind: Some(pb::request_command::Kind::AircraftEvent(
                        pb::AircraftEvent {
                            aircraft_id: Some(pb::Uuid {
                                value: vec![0x11; 16],
                            }),
                            info: Some(pb::AircraftCommandInfo {
                                kind: Some(pb::aircraft_command_info::Kind::Spawn(
                                    pb::AircraftSpawnInfo {
                                        name: "simulation-clock".to_string(),
                                        toml_config: String::new(),
                                        initial_state: Some(state),
                                        telemetry_schemas: Vec::new(),
                                    },
                                )),
                            }),
                        },
                    )),
                }),
            })),
        });
        let playback = PlaybackController::new(Arc::clone(&store), ReplayConfig::default());

        let frame = build_frame(&store, &playback, &playback.snapshot(), 0.5);

        assert_eq!(frame.aircraft.len(), 1);
        assert_eq!(frame.aircraft[0].aircraft_id, id);
        assert!(!frame.aircraft[0].stale);
        assert_eq!(frame.aircraft[0].source_timestamp_secs, 0.0);
    }

    #[test]
    fn frame_rejects_missing_zero_and_non_finite_pose() {
        assert!(!state_is_renderable(&pb::AircraftState::default()));
        let zero = pb::AircraftState {
            position: Some(pb::Vector3::default()),
            attitude: Some(pb::Quaternion::default()),
            ..pb::AircraftState::default()
        };
        assert!(!state_is_renderable(&zero));
        let non_finite = pb::AircraftState {
            position: Some(pb::Vector3 {
                x: f64::NAN,
                y: 0.0,
                z: 0.0,
            }),
            attitude: Some(pb::Quaternion {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            ..pb::AircraftState::default()
        };
        assert!(!state_is_renderable(&non_finite));
    }
}
