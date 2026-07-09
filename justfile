# FlyRuler Protocol — repository task runner

set fallback

_default:
    @just --list

# Install Python and Web dependencies.
setup: py-sync web-install

# Sync Python binding dependencies with uv.
py-sync:
    cd bindings/python && uv sync --all-groups

# Install Web console dependencies with pnpm.
web-install:
    cd web && pnpm install

# Format every language/tooling surface.
fmt: rust-fmt py-fmt web-fmt

# Check formatting for every language/tooling surface.
fmt-check: rust-fmt-check py-fmt-check web-fmt-check

# Format Rust sources.
rust-fmt:
    cargo fmt --all

# Check Rust formatting.
rust-fmt-check:
    cargo fmt --all --check

# Format Python binding sources and examples.
py-fmt:
    cd bindings/python && uv run ruff format src tests examples
    cd bindings/python && uv run ruff check --fix src tests examples

# Check Python binding formatting.
py-fmt-check:
    cd bindings/python && uv run ruff format --check src tests examples

# Format Web console sources.
web-fmt:
    cd web && pnpm format

# Check Web console formatting.
web-fmt-check:
    cd web && pnpm format:check

# Lint/check every language/tooling surface without running tests.
check: rust-check py-check web-check

# Run Rust formatting and clippy checks.
rust-check: rust-fmt-check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run Python formatting, lint, and bytecode checks.
py-check: py-fmt-check
    cd bindings/python && uv run ruff check src tests examples
    cd bindings/python && uv run python -m compileall -q src tests examples

# Run Web format, lint, test, type-check, and production build checks.
web-check:
    cd web && pnpm check

# Run every test suite.
test: rust-test py-test web-test

# Run Rust workspace tests.
rust-test:
    cargo test --workspace

# Build/install the Python extension locally and run Python tests.
py-test: py-develop
    cd bindings/python && uv run pytest tests/

# Run Web console unit tests.
web-test:
    cd web && pnpm test

# Build Rust workspace binaries/libraries.
build: rust-build web-build

# Build Rust workspace.
rust-build:
    cargo build --workspace

# Build/install the Python extension into the local uv environment.
py-develop:
    cd bindings/python && uv run maturin develop

# Build Web console production assets into web/dist.
web-build:
    cd web && pnpm build

# Cross-compile the MSFS 2024 bridge for Windows debug.
msfs-build:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc

# Cross-compile the MSFS 2024 bridge for Windows release.
msfs-build-release:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --release

# Run clippy for the Windows MSVC MSFS bridge target.
msfs-check:
    cargo xwin clippy -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --all-targets -- -D warnings

# Build the complete MSFS release bundle, including the Web console.
msfs-package: web-build msfs-build-release
    scripts/package_msfs_bundle.sh release dist/fly-ruler-msfs
    cd dist && rm -f fly-ruler-msfs-windows-x86_64.zip && zip -r fly-ruler-msfs-windows-x86_64.zip fly-ruler-msfs

# Run the standalone UDP + HTTP/WebSocket management daemon.
server-run *ARGS:
    cargo run -p fly_ruler_proto_server -- {{ARGS}}

# Run the Vue management console with the Vite development proxy.
web-run:
    cd web && pnpm dev

# Run backend and Vite development server together.
console-run *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -p fly_ruler_proto_server -- {{ARGS}} &
    server_pid=$!
    trap 'kill "${server_pid}" 2>/dev/null || true' EXIT INT TERM
    cd web
    pnpm dev

# Run the MSFS bridge inside the Steam MSFS 2024 Proton prefix.
msfs-run *ARGS:
    protontricks-launch --appid 2537590 target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe {{ARGS}}

# Run the geodetic MSFS demo sender.
demo-msfs *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_client.py {{ARGS}}

# Run the multi-aircraft MSFS AI demo sender.
demo-msfs-ai *ARGS:
    cd bindings/python && uv run python examples/demo_msfs_ai_client.py {{ARGS}}

# Run the standard pre-commit suite: format, lint/check, and tests.
pre-commit: fmt check test

# Run the local release confidence suite.
release-check: check test msfs-check msfs-package

# Update all project versions: Rust, protocol, Python, Web, lockfiles, docs.
version VERSION *ARGS:
    scripts/update_version.py {{VERSION}} {{ARGS}}
