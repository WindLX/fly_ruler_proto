use std::path::PathBuf;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let schema = manifest_dir.join("proto/fly_ruler.proto");

    println!("cargo:rerun-if-changed={}", schema.display());
    let include_dir = schema
        .parent()
        .expect("protobuf schema has a parent directory");

    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("failed to resolve bundled protoc binary path");
    // SAFETY: build scripts run in a single process context for this crate.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    prost_build::Config::new()
        .compile_protos(&[schema.as_path()], &[include_dir])
        .expect("failed to compile protobuf schema");
}
