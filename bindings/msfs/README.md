# FlyRuler MSFS 2024 Bridge

This crate builds a Windows sidecar that receives FlyRuler UDP state and drives the current Microsoft Flight Simulator 2024 user aircraft through SimConnect.
It is intended to run under the same Steam/Proton prefix as MSFS.

## SDK layout

Download the SDK from MSFS Developer Mode (`Help` → SDK installer). For the checked-in build scripts, extract SDK 1.6.9 at the workspace root:

```bash
mkdir -p .sdk-installer .msfs2024-sdk
unzip MSFS2024_SDK_Core_Installer_1.6.9.zip -d .sdk-installer
msiextract -C .msfs2024-sdk \
  .sdk-installer/MSFS2024_SDK_Core_Installer_1.6.9/MSFS2024_SDK_Core_Installer_1.6.9.msi
```

Alternatively set `MSFS2024_SDK` to the directory containing `SimConnect SDK/include` and `SimConnect SDK/lib`.

The SDK and installer directories are ignored by Git. `build.rs` verifies the official header and import library, links `SimConnect.lib`, and copies `SimConnect.dll` beside the bridge executable.

## Build on Linux

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build \
  -p fly_ruler_proto_msfs \
  --target x86_64-pc-windows-msvc
```

The resulting files are:

```text
target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe
target/x86_64-pc-windows-msvc/debug/SimConnect.dll
```

To build the same self-contained archive produced by CI:

```bash
just package-msfs
```

The output at `dist/fly-ruler-msfs-windows-x86_64.zip` includes the bridge, SimConnect runtime, example TOML, this README, the repository license, release guide, `SHA256SUMS`, and the tested production Web console under `web/dist`.

## Run

1. Start MSFS 2024 and enter a Free Flight with Active Pause disabled.
2. Start the bridge in the MSFS Proton prefix. For a release archive, first change into the extracted `fly-ruler-msfs` directory so the bundled `web/dist` is discovered automatically:

```bash
cd fly-ruler-msfs
uv tool install protontricks
protontricks-launch --appid 2537590 \
  ./fly-ruler-msfs-bridge.exe
```

   For a source-tree debug build, use `target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe` instead.

   `protontricks-launch` can emit a missing-`winetricks` warning when only its launcher functionality is used. This is non-fatal if the bridge proceeds to print `SimConnect connected`.

3. In another terminal, install the Python binding and start the demo:

   ```bash
   just develop
   cd bindings/python
   uv run python examples/demo_msfs_client.py
   ```

The bridge freezes latitude/longitude, altitude, and attitude only after the first valid FlyRuler state. A stale stream holds the final pose. Despawn, Ctrl-C, or normal shutdown restores the freeze state observed at startup.

Useful options:

```text
--config ./fly-ruler-msfs.toml
--listen 127.0.0.1:18002
--aircraft-id <32-character FlyRuler UUID>
--tick-hz 240
--stale-timeout-ms 500
--http-listen 127.0.0.1:18003
--data-root ./sessions
--web-root ./web/dist
--public-api-base-url https://sim.example.test/api/v1
--public-websocket-url wss://sim.example.test/api/v1/ws
--ws-hz 30
--cors-origin http://localhost:5173
--http
--no-http
--log-level info
--log-file ./logs/fly-ruler-msfs.log
```

Unless `--no-http` is supplied, the bridge embeds the same management service as `fly-ruler-server`. REST playback commands immediately affect SimConnect:
Live follows the newest sample, paused replay writes a seek exactly once, and playing replay advances using previous-value hold. A seek, load, clear, or other playback revision forces an explicit SimConnect refresh even when the selected sample has the same timestamp.

Release archives already contain `web/dist`; when launched from the extraction root, open `http://127.0.0.1:18003/` to use the management console. If the bridge is launched from another directory, pass `--web-root /absolute/path/to/fly-ruler-msfs/web/dist`.

## TOML configuration and logging

Copy `fly-ruler-msfs.example.toml` to `fly-ruler-msfs.toml`, or pass a custom path through `--config`. If `--config` is omitted, the bridge automatically loads `fly-ruler-msfs.toml` from the current directory when it exists.

Configuration precedence is CLI, then TOML, then built-in defaults. Relative `data_root`, `web_root`, and `logging.file_path` values are resolved relative to the TOML file. `--http` and `--no-http` explicitly override `management.enabled`.

`management.public_api_base_url` and `management.public_websocket_url` are optional. When omitted, the embedded Web console uses same-origin API paths. Set them only when the browser reaches the bridge through a reverse proxy or a different public host.

The bridge uses the same `LoggingConfig` and tracing subscriber as Core and the Python binding. `RUST_LOG` has priority over the configured log level. Without `logging.file_path`, structured logs are written to the terminal.

## State contract

- `derived.lat/lon`: WGS-84 decimal degrees.
- `derived.altitude`: MSL meters.
- `derived.tas`: true airspeed in meters/second.
- `derived.alpha/beta`: aerodynamic angles in radians.
- `attitude`: Hamilton quaternion `(w, x, y, z)`, body-FRD to local-NED.
- `angular_velocity`: body-FRD radians/second.
- `control_surfaces`: standard control-surface angles in radians and ratios in
  `0..=1`.
- `engines`: engine indices `1..=4` with throttle lever ratios in `0..=1`.

The bridge writes `AIRSPEED TRUE RAW`, reconstructed MSFS body-axis velocity, body angular rates, standard control-surface SimVars, and indexed engine throttles. MSFS derives IAS, Mach, angle of attack, and sideslip for its native instruments. `derived.ias/cas/mach` remain protocol telemetry and are not written directly.

## Landing-gear events

The bridge reserves two exact custom event names:

```text
flyruler.control.gear_up
flyruler.control.gear_down
```

They move the MSFS landing-gear handle through the `GEAR_UP` and `GEAR_DOWN` SimConnect events. No protobuf schema extension is required:

```python
client.create_event("flyruler.control.gear_up", timestamp=time.time())
client.create_event("flyruler.control.gear_down", timestamp=time.time())
```

Live mode applies newly received commands idempotently. Replay emits commands crossed by the global cursor in timestamp order. A seek, session load, playback revision, or aircraft reselection reapplies the last gear command at or before the cursor so the rendered aircraft matches the recorded timeline. If no gear event exists before the cursor, the bridge leaves the current MSFS gear state unchanged.

For an end-to-end visual test, run:

```bash
uv run python examples/demo_msfs_client.py --gear-cycle-secs 8
```

The CI/CD and artifact layout are documented in the repository root `RELEASING.md`; release archives include a copy beside this README.

For compatibility with older clients, these optional custom fields are used only when the corresponding `control_surfaces` field is absent:

```text
flyruler.control.aileron_left_rad
flyruler.control.aileron_right_rad
flyruler.control.elevator_rad
flyruler.control.rudder_rad
flyruler.control.flaps_left_ratio
flyruler.control.flaps_right_ratio
flyruler.control.spoilers_ratio
```

Ratios outside `0..=1`, engine indices outside `1..=4`, non-numeric values, and non-finite values are ignored independently without dropping the frame. Some complex third-party aircraft may override these SimVars; use a stock aircraft for the first integration test.
