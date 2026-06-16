use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;

use crate::config::LoggingConfig;
use tracing_subscriber::EnvFilter;

static LOGGING_INIT: OnceLock<()> = OnceLock::new();
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

const DEFAULT_LOG_LEVEL: &str = "warn";

fn build_default_filter(level: &str) -> String {
    format!(
        "{level},fly_ruler_proto_core.runtime=info,fly_ruler_proto_core.store=info,fly_ruler_proto_core.transport=warn"
    )
}

fn normalize_level(level: &str) -> &'static str {
    match level.to_ascii_lowercase().as_str() {
        "trace" => "trace",
        "debug" => "debug",
        "info" => "info",
        "warn" => "warn",
        "error" => "error",
        _ => DEFAULT_LOG_LEVEL,
    }
}

fn init_with_file(filter: EnvFilter, file_path: &str) {
    if let Some(parent) = Path::new(file_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }

    let file = match OpenOptions::new().create(true).append(true).open(file_path) {
        Ok(f) => f,
        Err(_) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(true)
                .with_thread_names(true)
                .try_init();
            return;
        }
    };

    let (writer, guard) = tracing_appender::non_blocking(file);
    let _ = LOG_GUARD.set(guard);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
}

/// Initialize tracing subscriber with runtime config once.
///
/// The first initialization wins; subsequent calls are no-ops.
pub fn init_logging(config: &LoggingConfig) {
    let _ = LOGGING_INIT.get_or_init(|| {
        let level = normalize_level(&config.level);
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(build_default_filter(level)));

        if let Some(path) = &config.file_path {
            init_with_file(filter, path);
        } else {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(true)
                .with_thread_names(true)
                .try_init();
        }
    });
}
