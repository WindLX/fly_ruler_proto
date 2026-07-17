use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use fly_ruler_proto_core::{
    LoggingConfig, ManagementConfig, ReplayConfig, RuntimeConfig, TransportConfig,
    RUNTIME_CONFIG_SCHEMA_VERSION,
};
use serde::Deserialize;

const DEFAULT_CONFIG_FILE: &str = "fly-ruler-server.toml";

#[derive(Debug, Parser)]
#[command(
    name = "fly-ruler-server",
    about = "FlyRuler UDP, HTTP, and WebSocket state server"
)]
pub struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    udp_listen: Option<String>,
    #[arg(long)]
    http_listen: Option<String>,
    #[arg(long)]
    data_root: Option<PathBuf>,
    #[arg(long)]
    web_root: Option<PathBuf>,
    #[arg(long)]
    public_api_base_url: Option<String>,
    #[arg(long)]
    public_websocket_url: Option<String>,
    #[arg(long)]
    ws_hz: Option<f64>,
    #[arg(long = "cors-origin")]
    cors_origins: Vec<String>,
    #[arg(long, conflicts_with = "no_http")]
    http: bool,
    #[arg(long, conflicts_with = "http")]
    no_http: bool,
    #[arg(long)]
    heartbeat_interval_secs: Option<u64>,
    #[arg(long)]
    heartbeat_timeout_secs: Option<u64>,
    #[arg(long)]
    playback_default_speed: Option<f64>,
    #[arg(long)]
    playback_min_speed: Option<f64>,
    #[arg(long)]
    playback_max_speed: Option<f64>,
    #[arg(long)]
    log_level: Option<String>,
    #[arg(long)]
    log_file: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    schema_version: Option<u32>,
    transport: TransportSection,
    management: ManagementSection,
    playback: PlaybackSection,
    logging: LoggingSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TransportSection {
    udp_listen: Option<String>,
    heartbeat_interval_secs: Option<u64>,
    heartbeat_timeout_secs: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ManagementSection {
    enabled: Option<bool>,
    listen: Option<String>,
    data_root: Option<PathBuf>,
    web_root: Option<PathBuf>,
    public_api_base_url: Option<String>,
    public_websocket_url: Option<String>,
    websocket_hz: Option<f64>,
    cors_origins: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PlaybackSection {
    default_speed: Option<f64>,
    min_speed: Option<f64>,
    max_speed: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LoggingSection {
    level: Option<String>,
    file_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub udp_listen: String,
    pub management_enabled: bool,
    pub management_listen: String,
    pub runtime: RuntimeConfig,
    pub config_path: Option<PathBuf>,
}

pub fn load() -> Result<ServerConfig, Box<dyn std::error::Error>> {
    resolve(Args::parse())
}

fn resolve(args: Args) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let config_path = args.config.clone().or_else(|| {
        let default = PathBuf::from(DEFAULT_CONFIG_FILE);
        default.is_file().then_some(default)
    });
    let file = match config_path.as_ref() {
        Some(path) => toml::from_str::<FileConfig>(&fs::read_to_string(path)?)?,
        None => FileConfig::default(),
    };
    let schema_version = file.schema_version.unwrap_or(RUNTIME_CONFIG_SCHEMA_VERSION);
    if schema_version != RUNTIME_CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {schema_version}; expected {RUNTIME_CONFIG_SCHEMA_VERSION}"
        )
        .into());
    }

    let base_dir = std::env::current_dir()?;
    let default_management = ManagementConfig::default();
    let default_transport = TransportConfig::default();
    let default_replay = ReplayConfig::default();
    let udp_listen = args
        .udp_listen
        .or(file.transport.udp_listen)
        .unwrap_or_else(|| "127.0.0.1:18002".to_string());
    let management_enabled = if args.http {
        true
    } else if args.no_http {
        false
    } else {
        file.management.enabled.unwrap_or(true)
    };
    let management_listen = args
        .http_listen
        .or(file.management.listen)
        .unwrap_or_else(|| "127.0.0.1:18003".to_string());
    let data_root = resolve_path(
        args.data_root
            .or(file.management.data_root)
            .unwrap_or_else(|| PathBuf::from("sessions")),
        &base_dir,
    );
    let web_root = resolve_path(
        args.web_root
            .or(file.management.web_root)
            .unwrap_or_else(|| PathBuf::from("web/dist")),
        &base_dir,
    );
    let websocket_hz = args
        .ws_hz
        .or(file.management.websocket_hz)
        .unwrap_or(default_management.websocket_hz);
    let cors_origins = if args.cors_origins.is_empty() {
        file.management
            .cors_origins
            .unwrap_or(default_management.cors_origins)
    } else {
        args.cors_origins
    };
    let heartbeat_interval_secs = args
        .heartbeat_interval_secs
        .or(file.transport.heartbeat_interval_secs)
        .unwrap_or(default_transport.heartbeat_interval_secs);
    let heartbeat_timeout_secs = args
        .heartbeat_timeout_secs
        .or(file.transport.heartbeat_timeout_secs)
        .unwrap_or(default_transport.heartbeat_timeout_secs);
    let default_speed = args
        .playback_default_speed
        .or(file.playback.default_speed)
        .unwrap_or(default_replay.default_speed);
    let min_speed = args
        .playback_min_speed
        .or(file.playback.min_speed)
        .unwrap_or(default_replay.min_speed);
    let max_speed = args
        .playback_max_speed
        .or(file.playback.max_speed)
        .unwrap_or(default_replay.max_speed);
    let logging = LoggingConfig {
        level: args
            .log_level
            .or(file.logging.level)
            .unwrap_or_else(|| "warn".to_string()),
        file_path: args
            .log_file
            .or(file.logging.file_path)
            .map(|path| resolve_path(path, &base_dir).to_string_lossy().to_string()),
    };

    if udp_listen.trim().is_empty() {
        return Err("transport.udp_listen must not be empty".into());
    }
    if management_enabled && management_listen.trim().is_empty() {
        return Err("management.listen must not be empty when management is enabled".into());
    }
    if heartbeat_interval_secs == 0 {
        return Err("transport.heartbeat_interval_secs must be greater than zero".into());
    }
    if heartbeat_timeout_secs <= heartbeat_interval_secs {
        return Err("transport.heartbeat_timeout_secs must exceed heartbeat_interval_secs".into());
    }
    if !websocket_hz.is_finite() || websocket_hz <= 0.0 {
        return Err("management.websocket_hz must be finite and greater than zero".into());
    }
    if !min_speed.is_finite()
        || !default_speed.is_finite()
        || !max_speed.is_finite()
        || min_speed <= 0.0
        || min_speed > default_speed
        || default_speed > max_speed
    {
        return Err(
            "playback speeds must be finite and satisfy 0 < min_speed <= default_speed <= max_speed"
                .into(),
        );
    }
    if !matches!(
        logging.level.as_str(),
        "trace" | "debug" | "info" | "warn" | "error"
    ) {
        return Err("logging.level must be trace, debug, info, warn, or error".into());
    }

    Ok(ServerConfig {
        udp_listen,
        management_enabled,
        management_listen,
        runtime: RuntimeConfig {
            transport: TransportConfig {
                heartbeat_interval_secs,
                heartbeat_timeout_secs,
            },
            management: ManagementConfig {
                data_root,
                web_root: Some(web_root),
                public_api_base_url: args
                    .public_api_base_url
                    .or(file.management.public_api_base_url),
                public_websocket_url: args
                    .public_websocket_url
                    .or(file.management.public_websocket_url),
                websocket_hz,
                cors_origins,
            },
            replay: ReplayConfig {
                default_speed,
                min_speed,
                max_speed,
            },
            logging,
            ..RuntimeConfig::default()
        },
        config_path,
    })
}

fn resolve_path(path: PathBuf, base_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        Args {
            config: None,
            udp_listen: None,
            http_listen: None,
            data_root: None,
            web_root: None,
            public_api_base_url: None,
            public_websocket_url: None,
            ws_hz: None,
            cors_origins: Vec::new(),
            http: false,
            no_http: false,
            heartbeat_interval_secs: None,
            heartbeat_timeout_secs: None,
            playback_default_speed: None,
            playback_min_speed: None,
            playback_max_speed: None,
            log_level: None,
            log_file: None,
        }
    }

    #[test]
    fn defaults_match_server_contract() {
        let config = resolve(args()).unwrap();
        assert_eq!(config.udp_listen, "127.0.0.1:18002");
        assert!(config.management_enabled);
        assert_eq!(config.management_listen, "127.0.0.1:18003");
        assert_eq!(config.runtime.transport.heartbeat_interval_secs, 5);
        assert_eq!(config.runtime.transport.heartbeat_timeout_secs, 15);
        assert_eq!(config.runtime.replay.default_speed, 1.0);
        assert_eq!(config.runtime.replay.min_speed, 0.1);
        assert_eq!(config.runtime.replay.max_speed, 16.0);
    }

    #[test]
    fn toml_populates_current_core_runtime_and_cli_wins() {
        let root = std::env::temp_dir().join(format!(
            "fly-ruler-server-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("server.toml");
        fs::write(
            &path,
            r#"
schema_version = 1

[transport]
udp_listen = "0.0.0.0:19002"
heartbeat_interval_secs = 2
heartbeat_timeout_secs = 9

[management]
enabled = false
listen = "0.0.0.0:19003"
data_root = "recordings"
web_root = "dashboard"
public_api_base_url = "https://example.test/api/v1"
public_websocket_url = "wss://example.test/api/v1/ws"
websocket_hz = 45.0
cors_origins = ["https://example.test"]

[playback]
default_speed = 2.0
min_speed = 0.25
max_speed = 8.0

[logging]
level = "debug"
file_path = "logs/server.log"
"#,
        )
        .unwrap();
        let mut cli = args();
        cli.config = Some(path);
        cli.udp_listen = Some("127.0.0.1:20002".to_string());
        cli.http = true;
        let config = resolve(cli).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(config.udp_listen, "127.0.0.1:20002");
        assert!(config.management_enabled);
        assert_eq!(config.management_listen, "0.0.0.0:19003");
        assert_eq!(config.runtime.transport.heartbeat_interval_secs, 2);
        assert_eq!(config.runtime.transport.heartbeat_timeout_secs, 9);
        assert_eq!(config.runtime.management.data_root, cwd.join("recordings"));
        assert_eq!(
            config.runtime.management.web_root,
            Some(cwd.join("dashboard"))
        );
        assert_eq!(config.runtime.management.websocket_hz, 45.0);
        assert_eq!(config.runtime.replay.default_speed, 2.0);
        assert_eq!(config.runtime.replay.min_speed, 0.25);
        assert_eq!(config.runtime.replay.max_speed, 8.0);
        assert_eq!(config.runtime.logging.level, "debug");
        assert_eq!(
            config.runtime.logging.file_path.as_deref(),
            Some(cwd.join("logs/server.log").to_string_lossy().as_ref())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_schema_and_invalid_runtime_values() {
        let root = std::env::temp_dir().join(format!(
            "fly-ruler-server-invalid-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("server.toml");
        fs::write(&path, "schema_version = 99\n").unwrap();
        let mut cli = args();
        cli.config = Some(path);
        assert!(resolve(cli).is_err());
        let _ = fs::remove_dir_all(root);

        let mut cli = args();
        cli.heartbeat_interval_secs = Some(10);
        cli.heartbeat_timeout_secs = Some(10);
        assert!(resolve(cli).is_err());

        let mut cli = args();
        cli.playback_min_speed = Some(2.0);
        cli.playback_max_speed = Some(1.0);
        assert!(resolve(cli).is_err());

        let mut cli = args();
        cli.ws_hz = Some(0.0);
        assert!(resolve(cli).is_err());

        let mut cli = args();
        cli.log_level = Some("verbose".to_string());
        assert!(resolve(cli).is_err());
    }
}
