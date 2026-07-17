# Fly Ruler Core Architecture (Current + Target)

## 1. Scope and Goals

This document is the architecture baseline for `core` and its direct runtime users
(`bindings/python`, `bindings/godot`, `bindings/msfs`).

v1 priorities:

- Keep UDP + protobuf runtime path stable and observable.
- Make kernel/server configuration explicit and complete.
- Keep runtime, transport, and store responsibilities separated.
- Keep API compatibility for current bindings while refactoring internals incrementally.

## 2. Current Architecture (As-Is)

### 2.1 Data Plane

- Wire payload: protobuf (`prost`) message.
- Runtime transport: UDP (`tokio::net::UdpSocket`).
- Session model: app-layer handshake/heartbeat with `client_uuid`.
- Persistence: transactional save/load using `meta.json` + parquet.
- Control plane: loopback-only Axum HTTP and read-only WebSocket snapshots.
- Playback: one global Live/Replay timeline using previous-value hold.
- Delivery: high-frequency state updates are best effort; no state ACK/retry in v1.

### 2.2 Core Modules

- `pb.rs`: generated protobuf module re-export.
- `transport.rs`: UDP client/server + session bookkeeping.
- `store.rs`: in-memory time-series storage and persistence.
- `playback.rs`: shared Live/Replay cursor, speed, and revision state.
- `management/`: REST/WebSocket runtime, ingestion gate, Series queries,
  Workspace persistence, global timeline events, and SPA hosting.
- `kernel.rs`: orchestration runtime (recv loop, ACK, ingestion, lifecycle).
- `logging.rs`: tracing subscriber initialization.
- `config.rs`: runtime configuration (transport/store/logging).
- `utils.rs`: internal helpers (`uuid_to_hex`, `now_secs`). **Not public API.**

> Note: `codec.rs` (length-delimited frame codec for an earlier TCP experiment) has
> been removed. The v1 transport is UDP-only.

### 2.3 Key Issues (Before Refactor)

- Transport layer currently carries part of session policy.
- Runtime ACK strategy was previously hardcoded.
- Configuration knobs were scattered or implicit.
- Historical document versions described TCP mainline; implementation is now UDP mainline.

## 3. Target Architecture (v1)

```text
bindings/python | bindings/godot | bindings/msfs | fly-ruler-server
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
            +--> PlaybackController (Live/Replay global cursor)
            |
            +--> Management Server (HTTP + read-only WebSocket)
            |       +--> Vue Web Console (runtime-configured SPA)
            |
            +--> Observability (structured tracing)
```

Design rules:

- `KernelRuntime` is the single orchestration entry.
- Transport focuses on message I/O and session primitives.
- ACK/session policy is fixed: handshake and heartbeat ACK; state updates do not.
- Store remains explicit and deterministic: no hidden autosave or interpolation.
- UDP ingestion continues during replay; save/load/clear use short maintenance gates.
- Management file access is restricted to validated names below a configured data root.
- Production Web assets are loaded from `web/dist`; Rust renders `index.html` with same-origin or explicitly configured public API/WS endpoints.
- Release CI builds `web/dist` once and injects that identical artifact into both the Linux daemon tarball and the MSFS bridge zip. The MSFS archive is verified to contain the executable, SimConnect runtime, Web entrypoint, hashed JS/CSS assets, documentation, license, and checksum manifest.
- Stored timestamps remain seconds. The Web console converts them to a global-origin relative axis while retaining absolute time for queries and seek.
- TOML configs, custom fields, and custom events are persisted as data, without schema validation.

The concrete tag workflow and distribution layouts are documented in [`RELEASING.md`](RELEASING.md).

## 4. Configuration Model

Configuration is explicit via strong Rust types.

- `RuntimeConfig`
  - `transport: TransportConfig`
  - `store: StoreConfig` (currently empty placeholder)
  - `management: ManagementConfig`
  - `replay: ReplayConfig`
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

### Phase 0: Baseline and Docs

- Align architecture docs with UDP/protobuf reality.
- Keep behavior stable while introducing config skeleton.

### Phase 1: Runtime Config Completion

- Keep `StoreConfig` as placeholder until knobs are finalized.
- Continue extending `RuntimeConfig` for logging/runtime limits.

### Phase 2: Responsibility Cleanup

- Reduce policy logic in transport.
- Move policy decisions into kernel/session orchestration.

### Phase 3: Reliability and Lifecycle

- Gracefully stop UDP, HTTP, WebSocket, and persistence coordination.
- Preserve a consistent store snapshot with a short ingestion maintenance gate.

### Phase 4: Test and Docs Hardening

- Add config-focused tests and edge-case session tests.
- Keep `AGENTS.md` short and synchronize architecture details in module docs.

## 7. Non-Goals (Current Iteration)

- No additional protocol schema changes beyond the unreleased 0.3.0 cleanup.
- No immediate switch to a different primary transport.
- No hidden autosave behavior in core runtime.
- No interpolation, reverse playback, looping, authentication, or server-side
  chart rendering.
- No schema validation for the opaque `toml_config`; telemetry schemas are validated at the client and server boundaries.

## 8. Compatibility Notes

- Bindings use the same high-level kernel/runtime operations while exposing the final 0.3.0 state schema directly, without legacy state-field shims.
- Future published protocol changes should prefer additive fields and reserve removed protobuf field numbers and names.
- The MSFS binding is an out-of-process Windows sidecar. It reuses the UDP
  kernel under Proton and keeps SimConnect-specific FFI outside `core`.
- The Godot binding embeds one `KernelRuntime` on a dedicated worker thread. Its `FlyRulerRuntime` node publishes immutable, playback-revision-consistent frame snapshots on the Godot main thread and embeds the same Web management console as the daemon and MSFS bridge.
- Rendering interpolation, NED/FRD-to-engine coordinate conversion, model binding, HUD logic, and flight dynamics remain outside the Godot binding.
