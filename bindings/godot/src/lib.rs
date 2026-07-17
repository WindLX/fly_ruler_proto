mod model;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::{fs, io::Write};

use fly_ruler_proto_core::{pb, Attitude};
use fly_ruler_proto_core::{
    KernelRuntime, LoggingConfig, ManagementConfig, PlaybackStepDirection, PlaybackStepUnit,
    ReplayConfig, RuntimeConfig, TimeSeriesStore, TransportConfig,
};
use godot::classes::ProjectSettings;
use godot::prelude::*;
use model::{
    build_frame, AircraftSnapshotData, FrameSnapshotData, GodotRuntimeFileConfig, RuntimeSettings,
};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

fn load_config_file(path: &str) -> Result<GodotRuntimeFileConfig, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("configuration file exceeds 64 KiB".to_string());
    }
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: GodotRuntimeFileConfig =
        toml::from_str(&source).map_err(|error| error.to_string())?;
    if file.schema_version != fly_ruler_proto_core::RUNTIME_CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {}; expected {}",
            file.schema_version,
            fly_ruler_proto_core::RUNTIME_CONFIG_SCHEMA_VERSION
        ));
    }
    Ok(file)
}

fn save_config_file(path: &PathBuf, file: &GodotRuntimeFileConfig) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let source = toml::to_string_pretty(file).map_err(|error| error.to_string())?;
    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err("serialized configuration exceeds 64 KiB".to_string());
    }
    let temporary = path.with_extension("toml.tmp");
    let mut output = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    output
        .write_all(source.as_bytes())
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl RuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

enum Command {
    Live,
    Pause,
    Seek(f64),
    Play(f64),
    SetSpeed(f64),
    Step(PlaybackStepUnit, PlaybackStepDirection, usize),
    Save(u64, PathBuf),
    Load(u64, PathBuf),
    Clear(u64),
    Shutdown,
}

enum WorkerEvent {
    Started {
        udp: String,
        management: String,
    },
    Sessions(Vec<(String, String, f64)>),
    OperationCompleted {
        id: u64,
        success: bool,
        error: String,
    },
    CommandError(String),
    Error(String),
    Stopped,
}

#[derive(GodotClass)]
#[class(base = RefCounted)]
struct FlyRulerRuntimeConfig {
    #[var]
    udp_listen: GString,
    #[var]
    management_enabled: bool,
    #[var]
    management_listen: GString,
    #[var]
    data_root: GString,
    #[var]
    web_root: GString,
    #[var]
    websocket_hz: f64,
    #[var]
    snapshot_hz: f64,
    #[var]
    heartbeat_interval_secs: i64,
    #[var]
    heartbeat_timeout_secs: i64,
    #[var]
    replay_min_speed: f64,
    #[var]
    replay_max_speed: f64,
    #[var]
    stale_timeout_secs: f64,
    #[var]
    log_level: GString,
    #[var]
    log_file: GString,
    config_error: GString,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for FlyRulerRuntimeConfig {
    fn init(base: Base<RefCounted>) -> Self {
        let file = GodotRuntimeFileConfig::default();
        Self {
            udp_listen: GString::from(&file.transport.udp_listen),
            management_enabled: file.management.enabled,
            management_listen: GString::from(&file.management.listen),
            data_root: GString::from(&file.management.data_root),
            web_root: GString::from(&file.management.web_root),
            websocket_hz: file.management.websocket_hz,
            snapshot_hz: file.visualization.snapshot_hz,
            heartbeat_interval_secs: file.transport.heartbeat_interval_secs as i64,
            heartbeat_timeout_secs: file.transport.heartbeat_timeout_secs as i64,
            replay_min_speed: file.playback.min_speed,
            replay_max_speed: file.playback.max_speed,
            stale_timeout_secs: file.visualization.stale_timeout_secs,
            log_level: GString::from(&file.logging.level),
            log_file: GString::from(&file.logging.file_path),
            config_error: GString::new(),
            base,
        }
    }
}

impl FlyRulerRuntimeConfig {
    fn globalize(path: &GString) -> String {
        if path.is_empty() {
            String::new()
        } else {
            ProjectSettings::singleton()
                .globalize_path(path)
                .to_string()
        }
    }

    fn resolve(&self) -> Result<RuntimeSettings, String> {
        let settings = RuntimeSettings {
            udp_listen: self.udp_listen.to_string(),
            management_enabled: self.management_enabled,
            management_listen: self.management_listen.to_string(),
            data_root: Self::globalize(&self.data_root),
            web_root: Self::globalize(&self.web_root),
            websocket_hz: self.websocket_hz,
            snapshot_hz: self.snapshot_hz,
            heartbeat_interval_secs: u64::try_from(self.heartbeat_interval_secs)
                .map_err(|_| "heartbeat_interval_secs must be positive".to_string())?,
            heartbeat_timeout_secs: u64::try_from(self.heartbeat_timeout_secs)
                .map_err(|_| "heartbeat_timeout_secs must be positive".to_string())?,
            replay_min_speed: self.replay_min_speed,
            replay_max_speed: self.replay_max_speed,
            stale_timeout_secs: self.stale_timeout_secs,
            log_level: self.log_level.to_string(),
            log_file: (!self.log_file.is_empty()).then(|| Self::globalize(&self.log_file)),
        };
        settings.validate()?;
        settings.validate_paths()?;
        Ok(settings)
    }

    fn apply_file(&mut self, file: GodotRuntimeFileConfig) {
        self.udp_listen = GString::from(&file.transport.udp_listen);
        self.heartbeat_interval_secs = file.transport.heartbeat_interval_secs as i64;
        self.heartbeat_timeout_secs = file.transport.heartbeat_timeout_secs as i64;
        self.management_enabled = file.management.enabled;
        self.management_listen = GString::from(&file.management.listen);
        self.data_root = GString::from(&file.management.data_root);
        self.web_root = GString::from(&file.management.web_root);
        self.websocket_hz = file.management.websocket_hz;
        self.snapshot_hz = file.visualization.snapshot_hz;
        self.stale_timeout_secs = file.visualization.stale_timeout_secs;
        self.replay_min_speed = file.playback.min_speed;
        self.replay_max_speed = file.playback.max_speed;
        self.log_level = GString::from(&file.logging.level);
        self.log_file = GString::from(&file.logging.file_path);
    }

    fn as_file(&self) -> GodotRuntimeFileConfig {
        let mut file = GodotRuntimeFileConfig::default();
        file.transport.udp_listen = self.udp_listen.to_string();
        file.transport.heartbeat_interval_secs = self.heartbeat_interval_secs.max(0) as u64;
        file.transport.heartbeat_timeout_secs = self.heartbeat_timeout_secs.max(0) as u64;
        file.management.enabled = self.management_enabled;
        file.management.listen = self.management_listen.to_string();
        file.management.data_root = self.data_root.to_string();
        file.management.web_root = self.web_root.to_string();
        file.management.websocket_hz = self.websocket_hz;
        file.visualization.snapshot_hz = self.snapshot_hz;
        file.visualization.stale_timeout_secs = self.stale_timeout_secs;
        file.playback.min_speed = self.replay_min_speed;
        file.playback.max_speed = self.replay_max_speed;
        file.logging.level = self.log_level.to_string();
        file.logging.file_path = self.log_file.to_string();
        file
    }
}

#[godot_api]
impl FlyRulerRuntimeConfig {
    #[func]
    fn load_toml(&mut self, path: GString) -> bool {
        self.config_error = GString::new();
        let absolute = Self::globalize(&path);
        let result = load_config_file(&absolute);
        match result {
            Ok(file) => {
                self.apply_file(file);
                true
            }
            Err(error) => {
                self.config_error = GString::from(&error);
                false
            }
        }
    }

    #[func]
    fn save_toml(&mut self, path: GString) -> bool {
        self.config_error = GString::new();
        let result = (|| -> Result<(), String> {
            self.resolve()?;
            let absolute = PathBuf::from(Self::globalize(&path));
            save_config_file(&absolute, &self.as_file())
        })();
        if let Err(error) = result {
            self.config_error = GString::from(&error);
            return false;
        }
        true
    }

    #[func]
    fn validate(&mut self) -> GString {
        match self.resolve() {
            Ok(_) => {
                self.config_error = GString::new();
                GString::new()
            }
            Err(error) => {
                self.config_error = GString::from(&error);
                GString::from(&error)
            }
        }
    }

    #[func]
    fn reset_defaults(&mut self) {
        self.apply_file(GodotRuntimeFileConfig::default());
        self.config_error = GString::new();
    }

    #[func]
    fn last_error(&self) -> GString {
        self.config_error.clone()
    }
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
struct FlyRulerAircraftSnapshot {
    #[var]
    aircraft_id: GString,
    #[var]
    name: GString,
    #[var]
    toml_config: GString,
    #[var]
    source_timestamp_secs: f64,
    #[var]
    spawned: bool,
    #[var]
    stale: bool,
    #[var]
    position_ned_m: Vector3,
    #[var]
    velocity_frd_mps: Vector3,
    #[var]
    attitude_wxyz: Quaternion,
    #[var]
    angular_velocity_frd_radps: Vector3,
    #[var]
    linear_acceleration_frd_mps2: Vector3,
    #[var]
    derived: VarDictionary,
    #[var]
    control_surfaces: VarDictionary,
    #[var]
    propulsors: Array<VarDictionary>,
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
struct FlyRulerFrameSnapshot {
    #[var]
    mode: GString,
    #[var]
    cursor_secs: f64,
    #[var]
    has_cursor: bool,
    #[var]
    speed: f64,
    #[var]
    bounds_start_secs: f64,
    #[var]
    bounds_end_secs: f64,
    #[var]
    has_bounds: bool,
    #[var]
    revision: i64,
    #[var]
    generated_at_secs: f64,
    #[var]
    aircraft: Array<Gd<FlyRulerAircraftSnapshot>>,
}

#[derive(GodotClass)]
#[class(base = Node)]
struct FlyRulerRuntime {
    base: Base<Node>,
    status: RuntimeStatus,
    udp_local_address: GString,
    management_local_address: GString,
    last_error: GString,
    latest_snapshot: Option<Gd<FlyRulerFrameSnapshot>>,
    active_sessions: Array<VarDictionary>,
    command_tx: Option<Sender<Command>>,
    event_rx: Option<Receiver<WorkerEvent>>,
    snapshot_rx: Option<Receiver<FrameSnapshotData>>,
    worker: Option<JoinHandle<()>>,
    next_operation_id: u64,
}

#[godot_api]
impl INode for FlyRulerRuntime {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            status: RuntimeStatus::Stopped,
            udp_local_address: GString::new(),
            management_local_address: GString::new(),
            last_error: GString::new(),
            latest_snapshot: None,
            active_sessions: Array::new(),
            command_tx: None,
            event_rx: None,
            snapshot_rx: None,
            worker: None,
            next_operation_id: 1,
        }
    }

    fn process(&mut self, _delta: f64) {
        self.drain_events();
        let mut newest = None;
        if let Some(rx) = &self.snapshot_rx {
            while let Ok(snapshot) = rx.try_recv() {
                newest = Some(snapshot);
            }
        }
        if let Some(snapshot) = newest {
            let value = frame_to_godot(snapshot);
            self.latest_snapshot = Some(value.clone());
            self.signals().snapshot_published().emit(&value);
        }
    }

    fn exit_tree(&mut self) {
        self.shutdown_internal();
    }
}

#[godot_api]
impl FlyRulerRuntime {
    #[signal]
    fn status_changed(status: GString);
    #[signal]
    fn snapshot_published(snapshot: Gd<FlyRulerFrameSnapshot>);
    #[signal]
    fn operation_completed(operation_id: i64, success: bool, error: GString);
    #[signal]
    fn runtime_error(error: GString);

    #[func]
    fn start(&mut self, config: Gd<FlyRulerRuntimeConfig>) -> bool {
        if self.status != RuntimeStatus::Stopped {
            self.report_error("runtime can only start from stopped state".to_string());
            return false;
        }
        let settings = match config.bind().resolve() {
            Ok(value) => value,
            Err(error) => {
                self.report_error(error);
                return false;
            }
        };
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::sync_channel(1);
        self.set_status(RuntimeStatus::Starting);
        self.command_tx = Some(command_tx);
        self.event_rx = Some(event_rx);
        self.snapshot_rx = Some(snapshot_rx);
        self.worker = Some(thread::spawn(move || {
            worker_main(settings, command_rx, event_tx, snapshot_tx)
        }));
        true
    }

    #[func]
    fn shutdown(&mut self) {
        self.shutdown_internal();
    }

    #[func]
    fn status(&self) -> GString {
        self.status.as_str().into()
    }
    #[func]
    fn udp_local_address(&self) -> GString {
        self.udp_local_address.clone()
    }
    #[func]
    fn management_local_address(&self) -> GString {
        self.management_local_address.clone()
    }
    #[func]
    fn last_error(&self) -> GString {
        self.last_error.clone()
    }
    #[func]
    fn latest_snapshot(&self) -> Option<Gd<FlyRulerFrameSnapshot>> {
        self.latest_snapshot.clone()
    }
    #[func]
    fn active_sessions(&self) -> Array<VarDictionary> {
        self.active_sessions.clone()
    }

    #[func]
    fn set_live(&mut self) -> bool {
        self.send(Command::Live)
    }
    #[func]
    fn pause(&mut self) -> bool {
        self.send(Command::Pause)
    }
    #[func]
    fn seek(&mut self, timestamp_secs: f64) -> bool {
        self.send(Command::Seek(timestamp_secs))
    }
    #[func]
    fn play(&mut self, speed: f64) -> bool {
        self.send(Command::Play(speed))
    }
    #[func]
    fn set_speed(&mut self, speed: f64) -> bool {
        self.send(Command::SetSpeed(speed))
    }
    #[func]
    fn step(&mut self, unit: GString, direction: GString, count: i64) -> bool {
        let unit = match unit.to_string().as_str() {
            "sample" => PlaybackStepUnit::Sample,
            "event" => PlaybackStepUnit::Event,
            _ => {
                self.report_error("step unit must be sample or event".to_string());
                return false;
            }
        };
        let direction = match direction.to_string().as_str() {
            "previous" => PlaybackStepDirection::Previous,
            "next" => PlaybackStepDirection::Next,
            _ => {
                self.report_error("step direction must be previous or next".to_string());
                return false;
            }
        };
        let Ok(count) = usize::try_from(count) else {
            self.report_error("step count must be positive".to_string());
            return false;
        };
        self.send(Command::Step(unit, direction, count))
    }

    #[func]
    fn save_session(&mut self, path: GString) -> i64 {
        self.session_command(path, Command::Save)
    }
    #[func]
    fn load_session(&mut self, path: GString) -> i64 {
        self.session_command(path, Command::Load)
    }
    #[func]
    fn clear_session(&mut self) -> i64 {
        let id = self.allocate_operation();
        if self.send(Command::Clear(id)) {
            id as i64
        } else {
            0
        }
    }
}

impl FlyRulerRuntime {
    fn set_status(&mut self, status: RuntimeStatus) {
        self.status = status;
        self.signals().status_changed().emit(status.as_str());
    }

    fn report_error(&mut self, error: String) {
        self.last_error = error.as_str().into();
        godot_error!("FlyRulerRuntime: {error}");
        self.signals().runtime_error().emit(error.as_str());
    }

    fn send(&mut self, command: Command) -> bool {
        if self.status != RuntimeStatus::Running {
            self.report_error("runtime command requires running state".to_string());
            return false;
        }
        if self
            .command_tx
            .as_ref()
            .is_some_and(|tx| tx.send(command).is_ok())
        {
            true
        } else {
            self.report_error("runtime worker is unavailable".to_string());
            false
        }
    }

    fn allocate_operation(&mut self) -> u64 {
        let id = self.next_operation_id;
        self.next_operation_id = self.next_operation_id.wrapping_add(1).max(1);
        id
    }

    fn session_command(
        &mut self,
        path: GString,
        make: impl FnOnce(u64, PathBuf) -> Command,
    ) -> i64 {
        let name = path.to_string();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            self.report_error(
                "session name must contain only ASCII letters, digits, '-' or '_'".to_string(),
            );
            return 0;
        }
        let id = self.allocate_operation();
        if self.send(make(id, PathBuf::from(name))) {
            id as i64
        } else {
            0
        }
    }

    fn drain_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                WorkerEvent::Started { udp, management } => {
                    self.udp_local_address = udp.as_str().into();
                    self.management_local_address = management.as_str().into();
                    self.set_status(RuntimeStatus::Running);
                }
                WorkerEvent::OperationCompleted { id, success, error } => {
                    self.signals()
                        .operation_completed()
                        .emit(id as i64, success, error.as_str());
                }
                WorkerEvent::Sessions(sessions) => {
                    let mut values = Array::new();
                    for (addr, client_uuid_hex, last_seen_secs) in sessions {
                        let mut value = VarDictionary::new();
                        value.set("addr", addr);
                        value.set("client_uuid_hex", client_uuid_hex);
                        value.set("last_seen_secs", last_seen_secs);
                        values.push(&value);
                    }
                    self.active_sessions = values;
                }
                WorkerEvent::CommandError(error) => self.report_error(error),
                WorkerEvent::Error(error) => {
                    self.report_error(error);
                    self.set_status(RuntimeStatus::Failed);
                }
                WorkerEvent::Stopped => self.set_status(RuntimeStatus::Stopped),
            }
        }
    }

    fn shutdown_internal(&mut self) {
        if matches!(
            self.status,
            RuntimeStatus::Stopped | RuntimeStatus::Stopping
        ) {
            return;
        }
        self.set_status(RuntimeStatus::Stopping);
        if let Some(tx) = self.command_tx.take() {
            let _ = tx.send(Command::Shutdown);
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                self.report_error("runtime worker panicked".to_string());
            }
        }
        self.event_rx = None;
        self.snapshot_rx = None;
        self.udp_local_address = GString::new();
        self.management_local_address = GString::new();
        self.set_status(RuntimeStatus::Stopped);
    }
}

fn worker_main(
    settings: RuntimeSettings,
    command_rx: Receiver<Command>,
    event_tx: Sender<WorkerEvent>,
    snapshot_tx: SyncSender<FrameSnapshotData>,
) {
    let async_runtime = match tokio::runtime::Runtime::new() {
        Ok(value) => value,
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
            return;
        }
    };
    let store = Arc::new(TimeSeriesStore::new());
    let config = RuntimeConfig {
        transport: TransportConfig {
            heartbeat_interval_secs: settings.heartbeat_interval_secs,
            heartbeat_timeout_secs: settings.heartbeat_timeout_secs,
        },
        management: ManagementConfig {
            data_root: settings.data_root.clone().into(),
            web_root: Some(settings.web_root.clone().into()),
            websocket_hz: settings.websocket_hz,
            ..ManagementConfig::default()
        },
        replay: ReplayConfig {
            default_speed: 1.0,
            min_speed: settings.replay_min_speed,
            max_speed: settings.replay_max_speed,
        },
        logging: LoggingConfig {
            level: settings.log_level.clone(),
            file_path: settings.log_file.clone(),
        },
        ..RuntimeConfig::default()
    };
    let mut kernel = KernelRuntime::with_config(Arc::clone(&store), config);
    let start_result: Result<(String, String), String> = async_runtime.block_on(async {
        kernel
            .start_server(&settings.udp_listen)
            .await
            .map_err(|error| error.to_string())?;
        let udp = kernel
            .udp_local_addr()
            .map_err(|error| error.to_string())?
            .to_string();
        let management = if settings.management_enabled {
            if let Err(error) = kernel
                .start_management_server(&settings.management_listen)
                .await
            {
                kernel.stop_server().await;
                return Err(error.to_string());
            }
            kernel
                .management_local_addr()
                .map_err(|error| error.to_string())?
                .to_string()
        } else {
            String::new()
        };
        Ok((udp, management))
    });
    let (udp, management) = match start_result {
        Ok(value) => value,
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::Error(error));
            return;
        }
    };
    let _ = event_tx.send(WorkerEvent::Started { udp, management });
    let playback = kernel.playback();
    let interval = Duration::from_secs_f64(1.0 / settings.snapshot_hz);
    let mut next_snapshot = Instant::now();
    let mut running = true;
    while running {
        let timeout = next_snapshot.saturating_duration_since(Instant::now());
        match command_rx.recv_timeout(timeout) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => running = false,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(command) => handle_command(
                command,
                &kernel,
                &playback,
                &event_tx,
                PathBuf::from(&settings.data_root).as_path(),
            ),
        }
        if running && Instant::now() >= next_snapshot {
            let state = playback.snapshot();
            let frame = build_frame(&store, &playback, &state, settings.stale_timeout_secs);
            match snapshot_tx.try_send(frame) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => running = false,
            }
            let sessions = async_runtime
                .block_on(kernel.active_sessions())
                .into_iter()
                .map(|session| {
                    (
                        session.addr.to_string(),
                        session.client_uuid_hex,
                        session.last_seen_secs,
                    )
                })
                .collect();
            let _ = event_tx.send(WorkerEvent::Sessions(sessions));
            next_snapshot = Instant::now() + interval;
        }
    }
    async_runtime.block_on(async {
        kernel.stop_management_server().await;
        kernel.stop_server().await;
    });
    let _ = event_tx.send(WorkerEvent::Stopped);
}

fn handle_command(
    command: Command,
    kernel: &KernelRuntime,
    playback: &fly_ruler_proto_core::PlaybackController,
    event_tx: &Sender<WorkerEvent>,
    data_root: &std::path::Path,
) {
    let timeline_result = match command {
        Command::Live => {
            playback.live();
            Some(Ok(()))
        }
        Command::Pause => Some(
            playback
                .pause()
                .map(|_| ())
                .map_err(|error| error.to_string()),
        ),
        Command::Seek(value) => Some(
            playback
                .seek(value)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        ),
        Command::Play(speed) => Some(
            playback
                .play(Some(speed))
                .map(|_| ())
                .map_err(|error| error.to_string()),
        ),
        Command::SetSpeed(speed) => Some(
            playback
                .set_speed(speed)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        ),
        Command::Step(unit, direction, count) => Some(
            playback
                .step(unit, direction, count)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        ),
        Command::Save(id, name) => {
            complete(
                event_tx,
                id,
                kernel
                    .save_session(&data_root.join(name))
                    .map_err(|error| error.to_string()),
            );
            None
        }
        Command::Load(id, name) => {
            complete(
                event_tx,
                id,
                kernel
                    .load_session(&data_root.join(name))
                    .map_err(|error| error.to_string()),
            );
            None
        }
        Command::Clear(id) => {
            kernel.clear_session();
            complete(event_tx, id, Ok(()));
            None
        }
        Command::Shutdown => None,
    };
    if let Some(Err(error)) = timeline_result {
        let _ = event_tx.send(WorkerEvent::CommandError(error));
    }
}

fn complete(event_tx: &Sender<WorkerEvent>, id: u64, result: Result<(), String>) {
    let (success, error) = match result {
        Ok(()) => (true, String::new()),
        Err(error) => (false, error),
    };
    let _ = event_tx.send(WorkerEvent::OperationCompleted { id, success, error });
}

fn frame_to_godot(data: FrameSnapshotData) -> Gd<FlyRulerFrameSnapshot> {
    Gd::from_init_fn(|_base| {
        let mut aircraft = Array::new();
        for item in data.aircraft {
            aircraft.push(&aircraft_to_godot(item));
        }
        FlyRulerFrameSnapshot {
            mode: GString::from(format!("{:?}", data.mode).to_snake_case().as_str()),
            cursor_secs: data.cursor_secs.unwrap_or_default(),
            has_cursor: data.cursor_secs.is_some(),
            speed: data.speed,
            bounds_start_secs: data.bounds.map_or(0.0, |value| value.0),
            bounds_end_secs: data.bounds.map_or(0.0, |value| value.1),
            has_bounds: data.bounds.is_some(),
            revision: data.revision as i64,
            generated_at_secs: data.generated_at_secs,
            aircraft,
        }
    })
}

fn aircraft_to_godot(data: AircraftSnapshotData) -> Gd<FlyRulerAircraftSnapshot> {
    Gd::from_init_fn(|_base| {
        let state = data.state;
        FlyRulerAircraftSnapshot {
            aircraft_id: GString::from(data.aircraft_id.as_str()),
            name: GString::from(data.name.as_str()),
            toml_config: GString::from(data.toml_config.as_str()),
            source_timestamp_secs: data.source_timestamp_secs,
            spawned: true,
            stale: data.stale,
            position_ned_m: vector(state.position.as_ref()),
            velocity_frd_mps: vector(state.velocity.as_ref()),
            attitude_wxyz: quaternion(state.attitude.as_ref()),
            angular_velocity_frd_radps: vector(state.angular_velocity.as_ref()),
            linear_acceleration_frd_mps2: vector(state.linear_acceleration_body.as_ref()),
            derived: derived(state.derived.as_ref()),
            control_surfaces: controls(state.control_surfaces.as_ref()),
            propulsors: propulsors(&state.propulsors),
        }
    })
}

fn vector(value: Option<&pb::Vector3>) -> Vector3 {
    value.map_or(Vector3::ZERO, |v| {
        Vector3::new(v.x as f32, v.y as f32, v.z as f32)
    })
}

fn quaternion(value: Option<&pb::Quaternion>) -> Quaternion {
    value
        .and_then(|value| Attitude::try_from(value).ok())
        .map_or(Quaternion::IDENTITY, |value| {
            let [w, x, y, z] = value.quaternion();
            Quaternion::new(x as f32, y as f32, z as f32, w as f32)
        })
}

fn derived(value: Option<&pb::DerivedState>) -> VarDictionary {
    let mut out = VarDictionary::new();
    let Some(value) = value else {
        return out;
    };
    for (key, field) in [
        ("lat", Some(value.lat)),
        ("lon", Some(value.lon)),
        ("altitude", Some(value.altitude)),
        ("alpha", Some(value.alpha)),
        ("beta", Some(value.beta)),
        ("tas", Some(value.tas)),
        ("eas", Some(value.eas)),
        ("gamma", Some(value.gamma)),
        ("chi", Some(value.chi)),
        ("ias", value.ias),
        ("cas", value.cas),
        ("mach", value.mach),
        ("ground_speed", value.ground_speed),
        ("vertical_speed", value.vertical_speed),
        ("dynamic_pressure", value.dynamic_pressure),
        ("normal_load_factor", value.normal_load_factor),
    ] {
        if let Some(field) = field {
            out.set(key, field);
        }
    }
    out
}

fn controls(value: Option<&pb::ControlSurfaceState>) -> VarDictionary {
    let mut out = VarDictionary::new();
    let Some(value) = value else {
        return out;
    };
    for (key, field) in [
        ("aileron_left_rad", value.aileron_left_rad),
        ("aileron_right_rad", value.aileron_right_rad),
        ("elevator_rad", value.elevator_rad),
        ("rudder_rad", value.rudder_rad),
        ("flaps_left_ratio", value.flaps_left_ratio),
        ("flaps_right_ratio", value.flaps_right_ratio),
        ("spoilers_ratio", value.spoilers_ratio),
    ] {
        if let Some(field) = field {
            out.set(key, field);
        }
    }
    out
}

fn propulsors(values: &[pb::PropulsorState]) -> Array<VarDictionary> {
    let mut out = Array::new();
    for value in values {
        let mut item = VarDictionary::new();
        item.set("propulsor_id", value.propulsor_id.as_str());
        item.set("kind", value.kind);
        if let Some(index) = value.index {
            item.set("index", index);
        }
        for (key, field) in [
            ("throttle_ratio", value.throttle_ratio),
            ("rpm", value.rpm),
            ("blade_pitch_rad", value.blade_pitch_rad),
            ("thrust_newton", value.thrust_newton),
            ("torque_newton_meter", value.torque_newton_meter),
        ] {
            if let Some(field) = field {
                item.set(key, field);
            }
        }
        out.push(&item);
    }
    out
}

trait SnakeCase {
    fn to_snake_case(self) -> String;
}
impl SnakeCase for String {
    fn to_snake_case(self) -> String {
        self.chars()
            .enumerate()
            .flat_map(|(index, ch)| {
                if ch.is_ascii_uppercase() && index > 0 {
                    vec!['_', ch.to_ascii_lowercase()]
                } else {
                    vec![ch.to_ascii_lowercase()]
                }
            })
            .collect()
    }
}

struct FlyRulerGodotExtension;

#[gdextension]
unsafe impl ExtensionLibrary for FlyRulerGodotExtension {}

#[cfg(test)]
mod config_file_tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("fly-ruler-godot-{name}-{nonce}"))
    }

    #[test]
    fn toml_defaults_partial_roundtrip_and_unknown_fields() {
        let root = test_root("roundtrip");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n[transport]\nudp_listen = \"127.0.0.1:19002\"\n",
        )
        .unwrap();
        let loaded = load_config_file(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.transport.udp_listen, "127.0.0.1:19002");
        assert_eq!(
            loaded.management,
            GodotRuntimeFileConfig::default().management
        );
        save_config_file(&path, &loaded).unwrap();
        assert_eq!(load_config_file(path.to_str().unwrap()).unwrap(), loaded);
        fs::write(&path, "schema_version = 1\nunknown = true\n").unwrap();
        assert!(load_config_file(path.to_str().unwrap()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_version_and_oversized_files() {
        let root = test_root("invalid");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, "schema_version = 2\n").unwrap();
        assert!(load_config_file(path.to_str().unwrap())
            .unwrap_err()
            .contains("schema_version"));
        fs::write(&path, "x".repeat(MAX_CONFIG_BYTES as usize + 1)).unwrap();
        assert!(load_config_file(path.to_str().unwrap())
            .unwrap_err()
            .contains("64 KiB"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_atomic_write_preserves_existing_file() {
        let root = test_root("atomic");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, "original").unwrap();
        fs::create_dir(path.with_extension("toml.tmp")).unwrap();
        assert!(save_config_file(&path, &GodotRuntimeFileConfig::default()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        fs::remove_dir_all(root).unwrap();
    }
}
