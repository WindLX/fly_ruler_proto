# CLAUDE.md

This file provides guidance to coding agents when working with this repository.

## Project Overview

Fly Ruler Protocol Kernel is a high-performance binary protocol library for aerospace flight simulation.

- Core wire format: protobuf payload framed as `[4-byte big-endian u32 length] + [N-byte protobuf]`
- Runtime transport: UDP (Tokio)
- Core serialization path: generated protobuf types + prost
- Primary current focus: core/runtime correctness + Python bindings lifecycle ergonomics

## Workspace Structure

```
fly_ruler_proto/
├── Cargo.toml                 # Workspace members: core, bindings/python
├── CLAUDE.md
├── prompt.md
├── fly_ruler.proto            # Protobuf schema source of truth
├── core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # Exports modules + PROTOCOL_VERSION
│       ├── codec.rs           # Frame codec (length-delimited protobuf)
│       ├── transport.rs       # UDP client/server runtime
│       ├── model.rs           # Rust domain models
│       ├── protocol.rs        # Protocol conversion helpers
│       ├── public_api.rs      # High-level API
│       ├── archivable.rs
│       ├── store.rs
│       └── archivable/
│           └── uuid.rs
└── bindings/
    └── python/
        ├── Cargo.toml
        ├── pyproject.toml
        ├── README.md
        ├── src/
        │   ├── lib.rs
        │   ├── client.rs      # PyO3 aircraft-bound client + server wrappers
        │   ├── protocol.rs    # Python exposed data structures
        │   └── fly_ruler_proto_python/
        │       ├── __init__.py
        │       ├── client.py  # High-level Python wrapper
        │       └── _core.pyi
        └── tests/
```

## Architecture Notes

### Core (Rust)

- Uses protobuf for runtime message encoding/decoding.
- Keeps framing compatibility through 4-byte BE length prefix.
- Uses UDP transport in `core/src/transport.rs`.
- Protocol version is centralized as `core::PROTOCOL_VERSION` and should be reused by bindings.

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

## Logging Requirements

- Client and server paths should provide structured logs.
- Bindings initialize tracing subscriber once (safe idempotent init).
- Prefer informative lifecycle logs (connect, spawn, update, heartbeat, despawn, close, errors).

## Build and Test Commands

```bash
# Core
cargo build -p fly_ruler_proto_core
cargo test -p fly_ruler_proto_core

# Python bindings crate tests (Rust side)
cargo test -p fly_ruler_proto_python

# Python package workflow (uv + maturin)
cd bindings/python
uv venv
source .venv/bin/activate
uv pip install maturin pytest
maturin develop
pytest tests/
```

## Technical Constraints

- Do not edit generated protobuf code manually.
- Keep wire framing stable.
- Use `thiserror`-based error handling in library code.
- Prefer `core::PROTOCOL_VERSION` as the single protocol version source.

## Current Priorities

1. Maintain compatibility and correctness for protobuf + UDP runtime path.
2. Keep Python API high-level and lifecycle-safe (minimal network ceremony for users).
3. Preserve and improve observability via logs.
4. Validate behavior with unit/integration tests after protocol or lifecycle changes.
