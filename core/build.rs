use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let package_schema = manifest_dir.join("proto/fly_ruler.proto");
    let workspace_schema = manifest_dir.join("../proto/fly_ruler.proto");

    println!("cargo:rerun-if-changed={}", package_schema.display());
    println!("cargo:rerun-if-changed={}", workspace_schema.display());

    let schema = if workspace_schema.is_file() {
        let workspace_contents =
            fs::read(&workspace_schema).expect("failed to read workspace protobuf schema");
        let package_contents =
            fs::read(&package_schema).expect("failed to read packaged protobuf schema mirror");
        assert_eq!(
            workspace_contents, package_contents,
            "core/proto/fly_ruler.proto must match proto/fly_ruler.proto"
        );
        workspace_schema
    } else {
        package_schema
    };
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
