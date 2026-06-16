fn main() {
    println!("cargo:rerun-if-changed=../proto/fly_ruler.proto");

    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("failed to resolve bundled protoc binary path");
    // SAFETY: build scripts run in a single process context for this crate.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    prost_build::Config::new()
        .compile_protos(&["../proto/fly_ruler.proto"], &["../proto"])
        .expect("failed to compile protobuf schema");
}
