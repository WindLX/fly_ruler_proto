# FlyRuler Protocol — repository task runner

set fallback

_default:
    @just --list

# Install Python and Web dependencies.
setup: setup-python setup-web

# Sync Python binding dependencies with uv.
setup-python:
    cd bindings/python && uv sync --all-groups

# Install Web console dependencies with pnpm.
setup-web:
    cd web && pnpm install

# Format every language/tooling surface.
fmt: fmt-rust fmt-python fmt-web

# Check formatting for every language/tooling surface.
check-format: check-format-rust check-format-python check-format-web

# Format Rust sources.
fmt-rust:
    cargo fmt --all

# Check Rust formatting.
check-format-rust:
    cargo fmt --all --check

# Format Python binding sources and examples.
fmt-python:
    cd bindings/python && uv run ruff format src tests examples
    cd bindings/python && uv run ruff check --fix src tests examples

# Check Python binding formatting.
check-format-python:
    cd bindings/python && uv run ruff format --check src tests examples

# Format Web console sources.
fmt-web:
    cd web && pnpm format

# Check Web console formatting.
check-format-web:
    cd web && pnpm format:check

# Lint/check every language/tooling surface without running tests.
check: check-rust check-python check-web

# Run Rust formatting and clippy checks.
check-rust: check-format-rust
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run Python formatting, lint, and bytecode checks.
check-python: check-format-python
    cd bindings/python && uv run ruff check src tests examples
    cd bindings/python && uv run python -m compileall -q src tests examples

# Run Web format, lint, test, type-check, and production build checks.
check-web:
    cd web && pnpm check

# Run every test suite.
test: test-rust test-python test-web

# Run Rust workspace tests.
test-rust:
    cargo test --workspace

# Build/install the Python extension locally and run Python tests.
test-python: build-python-dev
    cd bindings/python && uv run pytest tests/

# Run Web console unit tests.
test-web:
    cd web && pnpm test

# Build Rust workspace binaries/libraries.
build: build-rust build-web

# Build Rust workspace.
build-rust:
    cargo build --workspace

# Build/install the Python extension into the local uv environment.
build-python-dev:
    cd bindings/python && uv run maturin develop

# Build Web console production assets into web/dist.
build-web:
    cd web && pnpm build

# Cross-compile the MSFS 2024 bridge for Windows debug.
build-msfs:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc

# Cross-compile the MSFS 2024 bridge for Windows release.
build-msfs-release:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --release

# Run clippy for the Windows MSVC MSFS bridge target.
check-msfs:
    cargo xwin clippy -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --all-targets -- -D warnings

# Build the complete MSFS release bundle, including the Web console.
package-msfs: build-web build-msfs-release
    scripts/package_msfs_bundle.sh release dist/fly-ruler-msfs
    cd dist && rm -f fly-ruler-msfs-windows-x86_64.zip && zip -r fly-ruler-msfs-windows-x86_64.zip fly-ruler-msfs

# Run the standalone UDP + HTTP/WebSocket management daemon.
run-server *ARGS:
    cargo run -p fly_ruler_proto_server -- {{ARGS}}

# Run the Vue management console with the Vite development proxy.
dev-web:
    cd web && pnpm dev

# Run backend and Vite development server together.
dev-console *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -p fly_ruler_proto_server -- {{ARGS}} &
    server_pid=$!
    trap 'kill "${server_pid}" 2>/dev/null || true' EXIT INT TERM
    cd web
    pnpm dev

# Run the MSFS bridge inside the Steam MSFS 2024 Proton prefix.
run-msfs *ARGS:
    protontricks-launch --appid 2537590 target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe {{ARGS}}

# Run the geodetic MSFS demo sender.
example-msfs *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_client.py {{ARGS}}

# Run the multi-aircraft MSFS AI demo sender.
example-msfs-ai *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_ai_client.py {{ARGS}}

# Run the standard pre-commit suite: format, lint/check, and tests.
pre-commit: fmt check test

# Run the local release confidence suite.
check-release: check test check-msfs package-msfs

# Update all project versions: Rust, protocol, Python, Web, lockfiles, docs.
set-version VERSION *ARGS:
    scripts/update_version.py {{VERSION}} {{ARGS}}
