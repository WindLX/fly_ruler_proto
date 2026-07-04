use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn sdk_root(manifest_dir: &Path) -> PathBuf {
    env::var_os("MSFS2024_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            manifest_dir
                .join("../..")
                .join(".msfs2024-sdk")
                .join("MSFS 2024 SDK")
        })
}

fn main() {
    println!("cargo:rerun-if-env-changed=MSFS2024_SDK");
    println!("cargo:rerun-if-changed=build.rs");

    let target = env::var("TARGET").expect("TARGET is set by Cargo");
    if !target.contains("windows") {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let sdk = sdk_root(&manifest_dir);
    let include = sdk.join("SimConnect SDK/include/SimConnect.h");
    let lib_dir = sdk.join("SimConnect SDK/lib");
    let import_lib = lib_dir.join("SimConnect.lib");
    let dll = lib_dir.join("SimConnect.dll");

    for required in [&include, &import_lib, &dll] {
        assert!(
            required.is_file(),
            "missing MSFS 2024 SDK file: {} (set MSFS2024_SDK to the SDK root)",
            required.display()
        );
    }

    let header = fs::read_to_string(&include).expect("read SimConnect.h");
    for symbol in [
        "SimConnect_Open",
        "SimConnect_GetNextDispatch",
        "SimConnect_AddToDataDefinition",
        "SimConnect_SetDataOnSimObject",
        "SimConnect_TransmitClientEvent",
    ] {
        assert!(
            header.contains(symbol),
            "SimConnect.h does not contain required symbol {symbol}"
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=SimConnect");

    let workspace = manifest_dir.join("../..");
    let target_root = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    let profile = env::var("PROFILE").expect("PROFILE is set by Cargo");
    let output_dir = target_root.join(&target).join(profile);
    fs::create_dir_all(&output_dir).expect("create Windows target output directory");
    fs::copy(&dll, output_dir.join("SimConnect.dll"))
        .expect("copy SimConnect.dll beside bridge executable");
}
