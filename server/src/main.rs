use std::sync::Arc;

use fly_ruler_proto_core::{init_logging, KernelRuntime, TimeSeriesStore};
use tracing::info;

mod config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = config::load()?;
    init_logging(&settings.runtime.logging);
    info!(
        target: "fly_ruler_proto_server",
        config_path = ?settings.config_path,
        "server configuration loaded"
    );

    let store = Arc::new(TimeSeriesStore::new());
    let mut kernel = KernelRuntime::with_config(store, settings.runtime);
    kernel.start_server(&settings.udp_listen).await?;

    info!(
        target: "fly_ruler_proto_server",
        addr = %kernel.udp_local_addr()?,
        "FlyRuler UDP server started"
    );
    if settings.management_enabled {
        kernel
            .start_management_server(&settings.management_listen)
            .await?;
        info!(
            target: "fly_ruler_proto_server",
            addr = %kernel.management_local_addr()?,
            "FlyRuler HTTP/WebSocket management server started"
        );
    }

    tokio::signal::ctrl_c().await?;
    if settings.management_enabled {
        kernel.stop_management_server().await;
    }
    kernel.stop_server().await;
    Ok(())
}
