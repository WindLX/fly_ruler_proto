# Fly Ruler Core Architecture (Current + Target)

## 1. Scope and Goals

This document is the architecture baseline for `core` and its direct runtime users (`bindings/python`, `bindings/godot`).

Current priorities:

- Keep UDP + protobuf runtime path stable and observable.
- Make kernel/server configuration explicit and complete.
- Reduce module coupling and repetitive logic.
- Keep API compatibility for current bindings while refactoring internals incrementally.

## 2. Current Architecture (As-Is)

### 2.1 Data Plane

- Wire payload: protobuf (`prost`) message.
- Runtime transport: UDP (`tokio::net::UdpSocket`).
- Session model: app-layer handshake/heartbeat with `client_uuid`.
- Persistence: explicit `save_to_disk` / `load_from_disk` using `meta.json` + parquet.

### 2.2 Core Modules

- `pb.rs`: generated protobuf module re-export.
- `codec.rs`: length-delimited frame codec utility (legacy/TCP-oriented helper).
- `transport.rs`: UDP client/server + session bookkeeping.
- `store.rs`: in-memory time-series storage and persistence.
- `kernel.rs`: orchestration runtime (recv loop, ACK, ingestion, lifecycle).
- `logging.rs`: tracing subscriber initialization.
- `config.rs`: runtime configuration (newly introduced).

### 2.3 Key Issues (Before Refactor)

- Transport layer currently carries part of session policy.
- Runtime ACK strategy was previously hardcoded.
- Configuration knobs were scattered or implicit.
- Historical document versions described TCP mainline; implementation is now UDP mainline.

## 3. Target Architecture (To-Be)

```text
bindings/python | bindings/godot
            |
            v
      KernelRuntime API
            |
            +--> Session Policy (handshake/heartbeat/ack/prune)
            |
            +--> Transport I/O (UDP)
            |
            +--> Store (state/event + persistence)
            |
            +--> Observability (structured tracing)
```

Design rules:

- `KernelRuntime` is the single orchestration entry.
- Transport focuses on message I/O and connection/session primitives.
- ACK/session policy is fixed for correctness: handshake and heartbeat always ACK.
- Store remains explicit and deterministic (no hidden background autosave).

## 4. Configuration Model

Configuration is explicit via strong Rust types.

- `RuntimeConfig`
  - `transport: TransportConfig`
  - `store: StoreConfig` (currently empty placeholder)
  - `logging: LoggingConfig`

Planned extension points:

- `StoreConfig` concrete knobs (currently intentionally empty).
- `RuntimeLimits` (timeouts, queue sizes, datagram limits).

## 5. Logging and Observability

Current convention:

- targets:
  - `fly_ruler_proto_core.runtime`
  - `fly_ruler_proto_core.transport`
  - `fly_ruler_proto_core.store`
- default filter: global `warn`, selected modules at `info`.
- `RUST_LOG` always has higher priority than defaults.

Planned improvements:

- normalize key fields (`remote_addr`, `client_uuid`, `aircraft_id`, `msg_kind`).
- keep high-frequency path logs at `debug/trace` by default.
- retain state transitions and failures at `info/warn/error`.

## 6. Incremental Refactor Plan

### Phase 0: Baseline and Docs (in progress)

- Align architecture docs with UDP reality.
- Keep behavior stable while introducing config skeleton.

### Phase 1: Runtime Config Completion

- Keep `StoreConfig` as placeholder until knobs are finalized.
- Continue extending `RuntimeConfig` for logging/runtime limits.

### Phase 2: Responsibility Cleanup

- Reduce policy logic in transport.
- Move policy decisions into kernel/session orchestration.

### Phase 3: Reliability and Lifecycle

- Add graceful shutdown path instead of pure abort.
- Add bounded ingestion path for backpressure.

### Phase 4: Test and Docs Hardening

- Add config-focused tests and edge-case session tests.
- Ensure `CLAUDE.md` and module docs remain synchronized.

## 7. Non-Goals (Current Iteration)

- No protocol schema breaking changes.
- No immediate switch to a different primary transport.
- No hidden autosave behavior in core runtime.

## 8. Compatibility Notes

- Existing bindings should continue using the same high-level kernel/runtime operations.
- Internal refactor should prefer additive changes (`with_config`, new config types) before removals.
