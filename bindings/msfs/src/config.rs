use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use fly_ruler_proto_core::{LoggingConfig, ManagementConfig, RuntimeConfig};
use serde::Deserialize;

const DEFAULT_CONFIG_FILE: &str = "fly-ruler-msfs.toml";

#[derive(Debug, Parser)]
#[command(
    name = "fly-ruler-msfs-bridge",
    about = "Drive the MSFS 2024 user aircraft from FlyRuler UDP state"
)]
pub struct Args {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long)]
    pub aircraft_id: Option<String>,
    #[arg(long)]
    pub tick_hz: Option<f64>,
    #[arg(long)]
    pub stale_timeout_ms: Option<u64>,
    #[arg(long)]
    pub http_listen: Option<String>,
    #[arg(long)]
    pub data_root: Option<PathBuf>,
    #[arg(long)]
    pub web_root: Option<PathBuf>,
    #[arg(long)]
    pub public_api_base_url: Option<String>,
    #[arg(long)]
    pub public_websocket_url: Option<String>,
    #[arg(long)]
    pub ws_hz: Option<f64>,
    #[arg(long = "cors-origin")]
    pub cors_origins: Vec<String>,
    #[arg(long, conflicts_with = "no_http")]
    pub http: bool,
    #[arg(long, conflicts_with = "http")]
    pub no_http: bool,
    #[arg(long)]
    pub log_level: Option<String>,
    #[arg(long)]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    bridge: BridgeSection,
    management: ManagementSection,
    logging: LoggingSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BridgeSection {
    listen: Option<String>,
    aircraft_id: Option<String>,
    tick_hz: Option<f64>,
    stale_timeout_ms: Option<u64>,
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
    ws_hz: Option<f64>,
    cors_origins: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LoggingSection {
    level: Option<String>,
    file_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct BridgeConfig {
    pub listen: String,
    pub aircraft_id: Option<String>,
    pub tick_hz: f64,
    pub stale_timeout_ms: u64,
    pub management_enabled: bool,
    pub http_listen: String,
    pub runtime: RuntimeConfig,
    pub config_path: Option<PathBuf>,
}

pub fn load() -> Result<BridgeConfig, Box<dyn std::error::Error>> {
    resolve(Args::parse())
}

fn resolve(args: Args) -> Result<BridgeConfig, Box<dyn std::error::Error>> {
    let config_path = args.config.clone().or_else(|| {
        let default = PathBuf::from(DEFAULT_CONFIG_FILE);
        default.is_file().then_some(default)
    });
    let file = match config_path.as_ref() {
        Some(path) => {
            let source = fs::read_to_string(path)?;
            toml::from_str::<FileConfig>(&source)?
        }
        None => FileConfig::default(),
    };
    // All relative paths are resolved from the current working directory (the
    // directory from which the bridge was launched). This keeps TOML files
    // location-agnostic and matches user intuition: `web/dist` means "the
    // `web/dist` folder next to where I ran the executable".
    let base_dir = std::env::current_dir()?;

    let default_management = ManagementConfig::default();
    let listen = args
        .listen
        .or(file.bridge.listen)
        .unwrap_or_else(|| "127.0.0.1:18002".to_string());
    let aircraft_id = args.aircraft_id.or(file.bridge.aircraft_id);
    let tick_hz = args.tick_hz.or(file.bridge.tick_hz).unwrap_or(240.0);
    let stale_timeout_ms = args
        .stale_timeout_ms
        .or(file.bridge.stale_timeout_ms)
        .unwrap_or(500);
    let management_enabled = if args.http {
        true
    } else if args.no_http {
        false
    } else {
        file.management.enabled.unwrap_or(true)
    };
    let http_listen = args
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
    let websocket_hz = args.ws_hz.or(file.management.ws_hz).unwrap_or(30.0);
    let public_api_base_url = args
        .public_api_base_url
        .or(file.management.public_api_base_url);
    let public_websocket_url = args
        .public_websocket_url
        .or(file.management.public_websocket_url);
    let cors_origins = if args.cors_origins.is_empty() {
        file.management
            .cors_origins
            .unwrap_or(default_management.cors_origins)
    } else {
        args.cors_origins
    };
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

    validate(tick_hz, websocket_hz, aircraft_id.as_deref())?;
    Ok(BridgeConfig {
        listen,
        aircraft_id,
        tick_hz,
        stale_timeout_ms,
        management_enabled,
        http_listen,
        runtime: RuntimeConfig {
            management: ManagementConfig {
                data_root,
                web_root: Some(web_root),
                public_api_base_url,
                public_websocket_url,
                websocket_hz,
                cors_origins,
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

fn validate(
    tick_hz: f64,
    websocket_hz: f64,
    aircraft_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !tick_hz.is_finite() || tick_hz <= 0.0 {
        return Err("tick_hz must be finite and greater than zero".into());
    }
    if !websocket_hz.is_finite() || websocket_hz <= 0.0 {
        return Err("management.ws_hz must be finite and greater than zero".into());
    }
    if let Some(id) = aircraft_id {
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("aircraft_id must be a 32-character hexadecimal UUID".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        Args {
            config: None,
            listen: None,
            aircraft_id: None,
            tick_hz: None,
            stale_timeout_ms: None,
            http_listen: None,
            data_root: None,
            web_root: None,
            public_api_base_url: None,
            public_websocket_url: None,
            ws_hz: None,
            cors_origins: Vec::new(),
            http: false,
            no_http: false,
            log_level: None,
            log_file: None,
        }
    }

    #[test]
    fn defaults_match_cli_contract() {
        let config = resolve(args()).unwrap();
        assert_eq!(config.listen, "127.0.0.1:18002");
        assert_eq!(config.http_listen, "127.0.0.1:18003");
        assert_eq!(config.tick_hz, 240.0);
        assert!(config.management_enabled);
    }

    #[test]
    fn cli_values_override_defaults() {
        let mut cli = args();
        cli.tick_hz = Some(120.0);
        cli.no_http = true;
        cli.log_level = Some("debug".to_string());
        let config = resolve(cli).unwrap();
        assert_eq!(config.tick_hz, 120.0);
        assert!(!config.management_enabled);
        assert_eq!(config.runtime.logging.level, "debug");
    }

    #[test]
    fn validates_aircraft_id_and_rates() {
        let mut cli = args();
        cli.tick_hz = Some(0.0);
        assert!(resolve(cli).is_err());
        let mut cli = args();
        cli.aircraft_id = Some("bad".to_string());
        assert!(resolve(cli).is_err());
    }

    #[test]
    fn toml_paths_are_relative_to_config_and_cli_wins() {
        let root = std::env::temp_dir().join(format!(
            "fly-ruler-msfs-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("bridge.toml");
        fs::write(
            &path,
            r#"
[bridge]
tick_hz = 75.0

[management]
enabled = false
data_root = "recordings"
web_root = "dashboard"
public_api_base_url = "https://example.test/api/v1"
public_websocket_url = "wss://example.test/api/v1/ws"

[logging]
level = "debug"
file_path = "logs/bridge.log"
"#,
        )
        .unwrap();
        let mut cli = args();
        cli.config = Some(path.clone());
        cli.tick_hz = Some(120.0);
        cli.http = true;
        let config = resolve(cli).unwrap();
        assert_eq!(config.tick_hz, 120.0);
        assert!(config.management_enabled);
        // Relative paths are resolved from the current working directory, not
        // from the config file location.
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(config.runtime.management.data_root, cwd.join("recordings"));
        assert_eq!(
            config.runtime.management.web_root,
            Some(cwd.join("dashboard"))
        );
        assert_eq!(
            config.runtime.management.public_api_base_url.as_deref(),
            Some("https://example.test/api/v1")
        );
        assert_eq!(
            config.runtime.management.public_websocket_url.as_deref(),
            Some("wss://example.test/api/v1/ws")
        );
        assert_eq!(
            config.runtime.logging.file_path.as_deref(),
            Some(cwd.join("logs/bridge.log").to_string_lossy().as_ref())
        );
        let _ = fs::remove_dir_all(root);
    }
}
