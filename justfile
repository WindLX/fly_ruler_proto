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

# Run Rust workspace tests (excludes Python/Godot extension crates, which are
# tested through their respective language runtimes via `test-py`).
test-rs:
    cargo test --workspace

# Run Python binding tests
test-py: develop
    cd bindings/python && uv run pytest tests/

# Cross-compile the MSFS 2024 bridge and stage SimConnect.dll beside it
build-msfs:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc

# Build and stage the release MSFS bundle, including the production Web console
package-msfs: web-build
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --release
    scripts/package_msfs_bundle.sh release dist/fly-ruler-msfs
    cd dist && rm -f fly-ruler-msfs-windows-x86_64.zip && zip -r fly-ruler-msfs-windows-x86_64.zip fly-ruler-msfs

# Run the MSFS bridge inside the Steam MSFS 2024 Proton prefix
run-msfs *ARGS:
    protontricks-launch --appid 2537590 target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe {{ARGS}}

# Run the geodetic MSFS demo sender
demo-msfs *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_client.py {{ARGS}}

# Run the standalone UDP + HTTP/WebSocket management daemon
run-server *ARGS:
    cargo run -p fly_ruler_proto_server -- {{ARGS}}

# Run the Vue management console with the Vite development proxy
web-dev:
    cd web && pnpm dev

# Type-check and build the management console into web/dist
web-build:
    cd web && pnpm build

# Format the management console source code
web-fmt:
    cd web && pnpm format

# Run ESLint with auto-fix on the management console
web-lint-fix:
    cd web && pnpm lint --fix

# Lint, test, type-check, and build the management console
web-check:
    cd web && pnpm check

# Run the backend and Vite development server together
dev-console *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -p fly_ruler_proto_server -- {{ARGS}} &
    server_pid=$!
    trap 'kill "${server_pid}" 2>/dev/null || true' EXIT INT TERM
    cd web
    pnpm dev

# Run all static checks
_check-rs:
    cargo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

check: _check-rs web-check
    @echo "All checks passed"

# Auto-format Rust and frontend code
fmt:
    cargo fmt
    cd web && pnpm format
