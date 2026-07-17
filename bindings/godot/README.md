# fly_ruler_proto Godot runtime

`fly_ruler_proto_godot` is the in-process FlyRuler transport, storage, playback, and Web-management runtime for Godot 4. The 3D application consumes immutable frame snapshots and remains responsible for coordinate conversion, visual interpolation, models, cameras, and HUD rendering.

This release intentionally replaces the old synchronous `FlyRulerServer` API. Linux x86_64 is the only packaged platform.

## Runtime

Add a `FlyRulerRuntime` node to the scene tree, instantiate `FlyRulerRuntimeConfig`, connect the node signals, then call `start(config)`. Defaults are UDP `127.0.0.1:18002`, management `127.0.0.1:18003`, `user://sessions`, and the bundled Web console under `res://addons/fly_ruler_proto/web`.

The runtime emits:

- `status_changed(status)` with `stopped`, `starting`, `running`, `stopping`, or `failed`.
- `snapshot_published(snapshot)` on the Godot main thread.
- `operation_completed(id, success, error)` for asynchronous save/load/clear commands.
- `runtime_error(error)` for validation and command errors.

`FlyRulerFrameSnapshot` contains one playback mode, cursor, bounds, speed, revision, generation timestamp, and an array of `FlyRulerAircraftSnapshot` objects resolved at that exact playback snapshot. Despawned aircraft are excluded. State fields use SI, NED navigation axes, FRD body axes, and scalar-first wire quaternions projected to Godot's `(x,y,z,w)` `Quaternion` value.

In live mode, `stale` is based on the server's monotonic age since the last received state packet for that aircraft. `source_timestamp_secs` remains the producer-defined store/playback timeline and may be Unix time or zero-based simulation time; it is never compared with the local wall clock.

Timeline methods are `set_live`, `pause`, `seek`, `play`, `set_speed`, and `step`. Session methods return a non-zero operation ID: `save_session`, `load_session`, and `clear_session`.

## TOML configuration

`FlyRulerRuntimeConfig` can load, validate, and atomically save the versioned host configuration with `load_toml(path)`, `validate()`, `save_toml(path)`, `reset_defaults()`, and `last_error()`. Files are limited to 64 KiB, reject unknown fields, and contain `transport`, `management`, `visualization`, `playback`, and `logging` sections. Godot paths such as `user://sessions` and the bundled `res://addons/fly_ruler_proto/web` root are resolved before the worker starts.

The Godot host intentionally restricts management to loopback because the embedded console has no authentication or TLS. Runtime fields are startup configuration; applications should shut down and restart the runtime after saving changes. The tracing subscriber is process-wide, so changing logging output after the first runtime start requires restarting the Godot process.

## Install

From this repository:

```bash
bindings/godot/scripts/install_addon.sh /path/to/godot/project debug
```

The installer builds the Rust extension and Vue console, validates their artifacts, and installs the native library, Linux-only `.gdextension`, Web assets, version manifest, README, and a minimal typed GDScript example.
