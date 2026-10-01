# fly_ruler_proto Godot runtime

`fly_ruler_proto_godot` is the in-process FlyRuler transport, storage, playback, and Web-management runtime for Godot 4. The 3D application consumes immutable frame snapshots and remains responsible for coordinate conversion, visual interpolation, models, cameras, and HUD rendering.

This release intentionally replaces the old synchronous `FlyRulerServer` API. Linux x86_64 is the only packaged platform.

## Runtime

Add a `FlyRulerRuntime` node to the scene tree, instantiate `FlyRulerRuntimeConfig`, connect the node signals, then call `start(config)`. Server mode listens on UDP `0.0.0.0:18002`, management remains loopback-only at `127.0.0.1:18003`, cursor clients connect to `127.0.0.1:18002` by default, sessions use `user://sessions`, and the bundled Web console lives under `res://addons/fly_ruler_proto/web`.

The runtime emits:

- `status_changed(status)` with `stopped`, `starting`, `running`, `reconnecting`, `stopping`, or `failed`.
- `snapshot_published(snapshot)` on the Godot main thread.
- `stream_reset(reason)` before a client publishes a new epoch or playback revision.
- `cursor_events_published(events, baseline)` after reliable ordered events have been applied.
- `operation_completed(id, success, error)` for asynchronous save/load/clear commands.
- `runtime_error(error)` for validation and command errors.

`FlyRulerFrameSnapshot` contains one playback mode, cursor, bounds, speed, revision, generation timestamp, and an array of `FlyRulerAircraftSnapshot` objects resolved at that exact playback snapshot. Despawned aircraft are excluded. State fields use SI, NED navigation axes, FRD body axes, and scalar-first wire quaternions projected to Godot's `(x,y,z,w)` `Quaternion` value. `attitude_euler_rad` is a read-only `[roll, pitch, yaw]` projection produced by `fly_ruler_proto_core::Attitude`; it is not an input field and is not stored in `DerivedState`.

In live mode, `stale` is based on the server's monotonic age since the last received state packet for that aircraft. `source_timestamp_secs` remains the producer-defined store/playback timeline and may be Unix time or zero-based simulation time; it is never compared with the local wall clock.

Timeline methods are `set_live`, `pause`, `seek`, `play`, `set_speed`, and `step`. Session methods return a non-zero operation ID: `save_session`, `load_session`, and `clear_session`.

In `server` role the runtime owns the Kernel, store, management server, playback controller, and cursor publisher. In `client` role it creates an ephemeral UDP socket and publishes the same Godot snapshot type only after a reliable event baseline and complete keyframe arrive. Timeline/session commands return an explicit receive-only error. `stream_stats()` reports datagrams, accepted/dropped frames, reassembly failures, reliable batches, and observed retransmissions.

## TOML configuration

`FlyRulerRuntimeConfig` can load, validate, and atomically save schema 2 host configuration with `load_toml(path)`, `validate()`, `save_toml(path)`, `reset_defaults()`, and `last_error()`. Files are limited to 64 KiB, reject unknown fields, and contain `transport`, `cursor_stream`, `management`, `visualization`, `playback`, and `logging` sections. Schema 1 is migrated in memory to a `server` configuration. Godot paths such as `user://sessions` and the bundled `res://addons/fly_ruler_proto/web` root are resolved before the worker starts.

```toml
schema_version = 2

[transport]
role = "client"
udp_listen = "0.0.0.0:18002"
server_address = "192.168.1.20:18002"
heartbeat_interval_secs = 5
heartbeat_timeout_secs = 15

[cursor_stream]
publish_hz = 30.0
max_subscribers = 16
reconnect_initial_secs = 0.5
reconnect_max_secs = 5.0
```

Client configurations must disable management. Server management remains loopback-only and is never proxied by a cursor client.

The Godot host intentionally restricts management to loopback because the embedded console has no authentication or TLS. Runtime fields are startup configuration; applications should shut down and restart the runtime after saving changes. The tracing subscriber is process-wide, so changing logging output after the first runtime start requires restarting the Godot process.

## Install

From this repository:

```bash
bindings/godot/scripts/install_addon.sh /path/to/godot/project debug
```

The installer builds the Rust extension and Vue console, validates their artifacts, and installs the native library, Linux-only `.gdextension`, Web assets, version manifest, README, and a minimal typed GDScript example.

## Aircraft visualization profiles

The Godot binding also exposes `FlyRulerAircraftProfileConfig` for strict local visualization-profile TOML parsing. `load_toml(path)` enforces schema version 1, a 64 KiB limit, safe `res://` scene paths, the supported panel catalog, unique channels, finite ranges, and whitelisted snapshot data sources. `as_dictionary()` is intended for one centralized typed GDScript adapter; presentation code should not call the native object directly.
