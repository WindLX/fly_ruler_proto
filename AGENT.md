# CLAUDE.md

This file provides guidance to coding agents when working with this repository.

## Project Overview

Fly Ruler Protocol Kernel is a high-performance binary protocol and data kernel
for aerospace flight simulation visualization.

- Core wire format: protobuf UDP datagrams encoded with `prost`
- Runtime transport: UDP (Tokio)
- Core serialization path: generated protobuf types + prost
- Primary current focus: core/runtime correctness + Python bindings lifecycle ergonomics
- v1 runtime shape: embedded Rust library used from Python and Godot bindings

## Workspace Structure

```
fly_ruler_proto/
├── Cargo.toml                 # Workspace members: core, bindings/python, bindings/godot
├── CLAUDE.md
├── prompt.md
├── justfile                   # Unified build/test tasks
├── proto/
│   └── fly_ruler.proto        # Protobuf schema source of truth
├── core/
│   ├── Cargo.toml
│   ├── build.rs               # prost code generation
│   └── src/
│       ├── lib.rs             # Exports modules + PROTOCOL_VERSION
│       ├── transport.rs       # UDP client/server runtime
│       ├── config.rs          # Runtime/transport/store/logging configuration
│       ├── kernel.rs          # KernelRuntime orchestration
│       ├── logging.rs         # tracing subscriber initialization
│       ├── pb.rs              # Generated protobuf module
│       ├── store.rs           # Time-series storage and persistence
│       ├── utils.rs           # Internal helpers (uuid_to_hex, now_secs)
│       └── transport/
│           ├── client.rs
│           └── server.rs
└── bindings/
    ├── python/
    │   ├── Cargo.toml
    │   ├── pyproject.toml
    │   ├── README.md
    │   ├── .python-version    # "3.12"
    │   ├── uv.lock            # uv lockfile
    │   ├── examples/
    │   │   └── demo_client.py # Example client script
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── client.rs      # PyO3 aircraft-bound client + server wrappers
    │   │   ├── protocol.rs    # Python exposed data structures
    │   │   └── fly_ruler_proto_python/
    │   │       ├── __init__.py
    │   │       ├── client.py  # High-level Python wrapper
    │   │       └── _core.pyi
    │   └── tests/
    └── godot/
        ├── Cargo.toml
        ├── README.md
        ├── scripts/
        │   └── install_addon.sh
        ├── src/
        │   └── lib.rs
        └── templates/
            ├── ADDON_LAYOUT.md
            ├── FlyRulerDemo.gd
            └── fly_ruler_proto_godot.gdextension
```

## Architecture Notes

### Core (Rust)

- Uses protobuf for runtime message encoding/decoding.
- Uses UDP transport in `core/src/transport.rs`.
- Protocol version is centralized as `core::PROTOCOL_VERSION` and should be reused by bindings.
- `KernelRuntime` is the orchestration entry point for embedded server use.
- `TimeSeriesStore` owns latest/range queries plus explicit snapshot save/load.
- The kernel does not implement replay, interpolation, UI/model binding, or schema validation.
- `core/src/utils.rs` contains internal helpers and is **not** part of the public API.

### Session and Delivery Policy

- Handshake creates or replaces a session by `client_uuid`.
- Heartbeat refreshes `last_seen_secs`.
- Expired sessions are pruned by server receive activity.
- Handshake and heartbeat receive ACK responses.
- Protocol version mismatch receives an error response.
- State updates, spawn, despawn, and custom events are best effort in v1.

### Python Bindings (PyO3)

- `PyClient` is lifecycle-managed and aircraft-bound:
  - one client instance corresponds to one aircraft
  - constructor performs connect/handshake/spawn bootstrap
  - background tasks handle sender loop, operation loop, heartbeat loop
  - exposed operations are intentionally narrow: `update_state`, `create_event`, `despawn`, `close`
- `client.py` provides a user-friendly wrapper and context-manager semantics:
  - `with Client(...) as c:` auto cleanup on exit
  - client close path performs best-effort despawn + transport shutdown
- `PyServer` wraps UDP server receive/send and provides explicit `close()`.
- Python bindings share `core::logging::init_logging` for tracing subscriber initialization.

### Godot Binding

- `FlyRulerServer` wraps embedded `KernelRuntime`.
- Exposes start/stop, session inspection, aircraft IDs, latest/range queries, and explicit save/load.
- Godot owns replay timing, render interpolation, model loading, HUD, and UI behavior.

## Logging Requirements

- Client and server paths should provide structured logs.
- Bindings initialize tracing subscriber once (safe idempotent init) via `core::logging::init_logging`.
- Prefer informative lifecycle logs (connect, spawn, update, heartbeat, despawn, close, errors).

## Build and Test Commands

The `justfile` at the repository root provides the canonical entry points:

```bash
just setup        # Install Python dependencies via uv
just develop      # Build and install Python extension locally
just test         # Run Rust + Python tests
just test-rs      # cargo test --workspace
just test-py      # uv run pytest tests/ (inside bindings/python)
just check        # fmt + clippy + Python checks
just fmt          # cargo fmt
```

Manual equivalents:

```bash
# Core
cargo build -p fly_ruler_proto_core
cargo test -p fly_ruler_proto_core

# Python bindings crate tests (Rust side)
cargo test -p fly_ruler_proto_python

# Python package workflow (uv + maturin)
cd bindings/python
uv sync
uv run maturin develop
uv run pytest tests/
```

## Technical Constraints

- Do not edit generated protobuf code manually.
- Keep protobuf UDP wire behavior stable.
- Use `thiserror`-based error handling in library code.
- Prefer `core::PROTOCOL_VERSION` as the single protocol version source.
- Keep `toml_config`, `custom_fields`, and custom events as semantic pass-through data in core.
- Do not make `core::utils` public; keep it as internal helpers.

## Current Priorities

1. Maintain compatibility and correctness for protobuf + UDP runtime path.
2. Keep Python API high-level and lifecycle-safe (minimal network ceremony for users).
3. Preserve and improve observability via logs.
4. Validate behavior with unit/integration tests after protocol or lifecycle changes.
5. Keep docs synchronized with UDP/protobuf v1; TCP/bincode is not the current target.
