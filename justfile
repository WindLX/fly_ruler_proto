# Fly Ruler Protocol Kernel — unified build tasks

set fallback

_default:
    @just --list

# Install Python-side dependencies
setup:
    cd bindings/python && uv sync

# Build and install the Python extension locally
develop:
    cd bindings/python && uv run maturin develop

# Run the full local test suite (Rust + Python)
test: test-rs test-py

# Run Rust workspace tests
test-rs:
    cargo test --workspace

# Run Python binding tests
test-py: develop
    cd bindings/python && uv run pytest tests/

# Cross-compile the MSFS 2024 bridge and stage SimConnect.dll beside it
build-msfs:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc

# Run the MSFS bridge inside the Steam MSFS 2024 Proton prefix
run-msfs *ARGS:
    protontricks-launch --appid 2537590 target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe {{ARGS}}

# Run the geodetic MSFS demo sender
demo-msfs *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_client.py {{ARGS}}

# Run the standalone UDP + HTTP/WebSocket management daemon
run-server *ARGS:
    cargo run -p fly_ruler_proto_server -- {{ARGS}}

# Run all static checks
_check-rs:
    cargo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

check: _check-rs
    @echo "All checks passed"

# Auto-format Rust code
fmt:
    cargo fmt
