# FlyRuler Proto — repository task runner

set fallback

# Godot 4 编辑器可执行文件，godot-rust 在构建 `bindings/godot` 时需要它生成扩展 API。
export GDRUST_GODOT_BIN := env_var_or_default("GDRUST_GODOT_BIN", "/usr/bin/godot-mono")

_default:
    @just --list

# Install Python and Web dependencies.
setup: _setup-python _setup-web

# Format every surface in place.
fmt: _fmt-rust _fmt-python _fmt-web

# Check Rust, Python, and Web surfaces, plus version consistency.
check: _check-version _check-rust _check-python _check-web _check-scripts

# Run every test suite.
test: _test-rust _test-python _test-web

# Build the Rust workspace and the Web console assets.
build: _build-rust _build-web

# Run the server and the Web console: dev [all|server|web] *ARGS.
dev TARGET="all" *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{TARGET}}" in
      server)
        cargo run -p fly_ruler_proto_server -- {{ARGS}}
        ;;
      web)
        cd web && pnpm dev
        ;;
      all)
        cargo run -p fly_ruler_proto_server -- {{ARGS}} &
        server_pid=$!
        trap 'kill "${server_pid}" 2>/dev/null || true' EXIT INT TERM
        cd web
        pnpm dev
        ;;
      *)
        echo "未知目标：{{TARGET}}，可选 all、server、web" >&2
        exit 2
        ;;
    esac

# MSFS and Windows steps: msfs <check|build|build-release|package|run|example|example-ai> *ARGS.
msfs TASK *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{TASK}}" in
      check) just _msfs-check {{ARGS}} ;;
      build) just _msfs-build {{ARGS}} ;;
      build-release) just _msfs-build-release {{ARGS}} ;;
      package) just _msfs-package {{ARGS}} ;;
      run) just _msfs-run {{ARGS}} ;;
      example) just _msfs-example {{ARGS}} ;;
      example-ai) just _msfs-example-ai {{ARGS}} ;;
      *)
        echo "未知任务：{{TASK}}，可选 check、build、build-release、package、run、example、example-ai" >&2
        exit 2
        ;;
    esac

# Run the full local release confidence suite.
check-release: check test _msfs-check _msfs-package

# Update every version source and render the result: set-version X.Y.Z *ARGS.
set-version VERSION *ARGS:
    scripts/version.py set {{VERSION}} {{ARGS}}

# 只读列出全部版本源、锁文件携带情况与工具版本。
version:
    scripts/version.py show

# Run format, check, and tests together.
pre-commit: fmt check test

# ---------------------------------------------------------------------------
# Private recipes: single-surface entry points used by the aggregate commands.
# ---------------------------------------------------------------------------

_setup-python:
    cd bindings/python && uv sync --all-groups

_setup-web:
    cd web && pnpm install

_fmt-rust:
    cargo fmt --all

_fmt-python:
    cd bindings/python && uv run ruff format src tests examples
    cd bindings/python && uv run ruff check --fix src tests examples

_fmt-web:
    cd web && pnpm format

_check-version:
    python3 scripts/version.py check

_check-rust:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

_check-python:
    cd bindings/python && uv run ruff format --check src tests examples
    cd bindings/python && uv run ruff check src tests examples
    cd bindings/python && uv run mypy
    cd bindings/python && uv run python -m compileall -q src tests examples

_check-web:
    cd web && pnpm check

_check-scripts:
    bash -n scripts/install-msfs.sh scripts/package_msfs_bundle.sh
    python3 -m py_compile scripts/version.py

_test-rust:
    cargo test --workspace

_test-python: _build-python-dev
    cd bindings/python && uv run pytest tests/

_test-web:
    cd web && pnpm test

_build-rust:
    cargo build --workspace

_build-python-dev:
    cd bindings/python && uv run maturin develop

_build-web:
    cd web && pnpm build

_msfs-check:
    cargo xwin clippy -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --all-targets -- -D warnings

_msfs-build:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc

_msfs-build-release:
    cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc --release

_msfs-package: _build-web _msfs-build-release
    scripts/package_msfs_bundle.sh release dist/fly-ruler-msfs
    cd dist && rm -f fly-ruler-msfs-windows-x86_64.zip && zip -r fly-ruler-msfs-windows-x86_64.zip fly-ruler-msfs

_msfs-run *ARGS:
    protontricks-launch --appid 2537590 target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe --config {{env_var_or_default("FR_MSFS_CONFIG", "bindings/msfs/fly-ruler-msfs.dev.toml")}} {{ARGS}}

_msfs-example *ARGS:
    cd bindings/python && uv run python examples/02_control_msfs.py {{ARGS}}

_msfs-example-ai *ARGS:
    cd bindings/python && uv run python examples/06_ai_fleet_msfs.py {{ARGS}}
