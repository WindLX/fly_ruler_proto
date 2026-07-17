mod config;

#[cfg(windows)]
mod ai;
#[cfg(windows)]
mod aircraft;
#[cfg(windows)]
mod bridge;

#[cfg(not(windows))]
fn main() {
    let settings = config::load().ok();
    let logging = settings
        .as_ref()
        .map(|settings| &settings.runtime.logging)
        .cloned()
        .unwrap_or_default();
    fly_ruler_proto_core::init_logging(&logging);
    tracing::error!(
        target: "fly_ruler_proto_msfs.bridge",
        "fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton"
    );
    std::process::exit(2);
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    bridge::run(config::load()?)
}
