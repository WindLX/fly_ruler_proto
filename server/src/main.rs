use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use fly_ruler_proto_core::{KernelRuntime, ManagementConfig, RuntimeConfig, TimeSeriesStore};

#[derive(Debug, Parser)]
#[command(
    name = "fly-ruler-server",
    about = "FlyRuler UDP, HTTP, and WebSocket state server"
)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:18002")]
    udp_listen: String,

    #[arg(long, default_value = "127.0.0.1:18003")]
    http_listen: String,

    #[arg(long, default_value = "./sessions")]
    data_root: PathBuf,

    #[arg(long, default_value = "./web/dist")]
    web_root: PathBuf,

    #[arg(long)]
    public_api_base_url: Option<String>,

    #[arg(long)]
    public_websocket_url: Option<String>,

    #[arg(long, default_value_t = 30.0)]
    ws_hz: f64,

    #[arg(long = "cors-origin")]
    cors_origins: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let defaults = ManagementConfig::default();
    let config = RuntimeConfig {
        management: ManagementConfig {
            data_root: args.data_root,
            web_root: Some(args.web_root),
            public_api_base_url: args.public_api_base_url,
            public_websocket_url: args.public_websocket_url,
            websocket_hz: args.ws_hz,
            cors_origins: if args.cors_origins.is_empty() {
                defaults.cors_origins
            } else {
                args.cors_origins
            },
        },
        ..RuntimeConfig::default()
    };

    let store = Arc::new(TimeSeriesStore::new());
    let mut kernel = KernelRuntime::with_config(store, config);
    kernel.start_server(&args.udp_listen).await?;
    kernel.start_management_server(&args.http_listen).await?;

    println!("FlyRuler UDP listening on {}", kernel.udp_local_addr()?);
    println!(
        "FlyRuler HTTP/WebSocket listening on {}",
        kernel.management_local_addr()?
    );

    tokio::signal::ctrl_c().await?;
    kernel.stop_management_server().await;
    kernel.stop_server().await;
    Ok(())
}
