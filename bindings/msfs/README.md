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

Depend on llvm, for debian:
```bash
sudo apt-get update
sudo apt-get install -y llvm-18 zip unzip
sudo ln -sf /usr/bin/llvm-lib-18 /usr/local/bin/llvm-lib
command -v llvm-lib
```

for archlinux:
```bash
sudo pacman -S llvm llvm-libs
```

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
   just build-python-dev
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
--render-hz 240
--smoothing-mode low_latency
--interpolation-delay-ms 30
--max-extrapolation-ms 40
--stale-timeout-ms 500
--enable-ai-aircraft
--ai-aircraft-title "Rafale M"
--max-ai-aircraft 8
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

Live stale detection uses the core server's monotonic age since the last received state packet. The producer source timestamp is reserved for ordering, smoothing, and replay, so both Unix timestamps and zero-based simulation clocks are supported without comparing unrelated clocks.

Release archives already contain `web/dist`; when launched from the extraction root, open `http://127.0.0.1:18003/` to use the management console. If the bridge is launched from another directory, pass `--web-root /absolute/path/to/fly-ruler-msfs/web/dist`.

## TOML configuration and logging

Copy `fly-ruler-msfs.example.toml` to `fly-ruler-msfs.toml`, or pass a custom path through `--config`. If `--config` is omitted, the bridge automatically loads `fly-ruler-msfs.toml` from the current directory when it exists.

Configuration precedence is CLI, then TOML, then built-in defaults. Relative `data_root`, `web_root`, and `logging.file_path` values are resolved from the directory where the bridge is launched. `--http` and `--no-http` explicitly override `management.enabled`.

## Live smoothing

Live mode uses a bridge-side sample buffer so MSFS receives a fixed-cadence, coherent state frame instead of jittery latest-sample steps. This reduces visual and HUD jumps when UDP arrival timing, controller timing, Proton scheduling, and SimConnect rendering do not line up exactly.

The smoothing modes are:

- `low_latency` (default): 30 ms interpolation delay and up to 40 ms short extrapolation. This is intended for hand-flown closed-loop control.
- `smooth`: 80 ms interpolation delay and up to 20 ms short extrapolation. Use this when visual smoothness matters more than response latency.
- `latest`: old compatibility behavior. The bridge writes only newly received latest samples, which is useful for A/B tests and diagnosing whether a symptom is caused by smoothing or by source data.

`render_hz` controls the SimConnect write cadence and defaults to `tick_hz`. `interpolation_delay_ms` and `max_extrapolation_ms` can override the preset values. When the stream becomes stale, the bridge stops extrapolating and holds the last valid pose; despawn, disconnect, or normal shutdown still releases the MSFS freeze state.

`management.public_api_base_url` and `management.public_websocket_url` are optional. When omitted, the embedded Web console uses same-origin API paths. Set them only when the browser reaches the bridge through a reverse proxy or a different public host.

The bridge uses the same `LoggingConfig` and tracing subscriber as Core and the Python binding. `RUST_LOG` has priority over the configured log level. Without `logging.file_path`, structured logs are written to the terminal.

## Multi-aircraft AI rendering

By default the bridge keeps the original single-aircraft behavior. To render additional FlyRuler aircraft, enable AI rendering:

```bash
./fly-ruler-msfs-bridge.exe \
  --aircraft-id <main FlyRuler UUID> \
  --enable-ai-aircraft \
  --ai-aircraft-title "Rafale M" \
  --max-ai-aircraft 8
```

The selected aircraft is still mapped to the MSFS user aircraft and keeps the native HUD/cockpit path. Other spawned FlyRuler aircraft are created through SimConnect as non-ATC AI aircraft, released from the MSFS AI controller, frozen, and then updated at the same render cadence as the user aircraft.

`ai_aircraft_title` must match an aircraft preset/container title available in MSFS, for example a stock title or another installed aircraft that works as an AI object. The MVP uses one shared title for all AI aircraft. Complex third-party aircraft may not animate every cockpit, engine, or surface system correctly when instantiated as AI; use AI rendering primarily as an external visual/formation view.

If SimConnect reports exception `22` / `CREATE_OBJECT_FAILED`, the bridge keeps running and retries later, but MSFS rejected the requested AI aircraft. The most common cause is an unavailable or non-AI-compatible `ai_aircraft_title`. On a Proton install, you can inspect Community aircraft titles with:

```bash
find "$HOME/.local/share/Steam/steamapps/compatdata/2537590/pfx/drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024/Packages/Community" \
  -name aircraft.cfg -print0 |
  xargs -0 awk -F= 'tolower($1) ~ /^[[:space:]]*title[[:space:]]*$/ { print $2 }'
```

Then launch the bridge with a discovered title, for example:

```bash
./fly-ruler-msfs-bridge.exe \
  --enable-ai-aircraft \
  --ai-aircraft-title "Rafale M"
```

The AI object's tail number is a conservative `FR` prefix plus the first eight hexadecimal characters of the FlyRuler aircraft id. Some aircraft or SimConnect builds reject longer or more descriptive tail numbers during AI creation, so the bridge keeps this value deliberately simple. Use the FlyRuler Web console or bridge logs for full aircraft names and ids.

Despawn, session clear/load, replay seek, and bridge shutdown remove AI objects created by the bridge. The bridge can only remove objects it created itself.

## State contract

- `derived.lat/lon`: WGS-84 decimal degrees.
- `derived.altitude`: MSL meters.
- `velocity`: body-FRD velocity in meters/second (`x` forward, `y` right,
  `z` down).
- `derived.tas` and `derived.alpha/beta`: telemetry only for this bridge; they
  are not used to reconstruct MSFS body velocity.
- `attitude`: Hamilton quaternion `(w, x, y, z)`, body-FRD to local-NED.
- `angular_velocity`: body-FRD radians/second.
- `control_surfaces`: standard control-surface angles in radians and ratios in
  `0..=1`.
- `propulsors`: stable `propulsor_id` values with optional simulator `index`; entries with index `1..=4` and `throttle_ratio` in `0..=1` drive the corresponding MSFS engine slot.

The bridge writes MSFS body-axis velocity, body angular rates, standard control-surface SimVars, and indexed `GENERAL ENG THROTTLE LEVER POSITION` SimVars. It reads control surfaces only from `ControlSurfaceState`; model diagnostics belong in schema-first telemetry. It no longer writes `AIRSPEED TRUE RAW`, and it no longer reconstructs velocity from angle of attack, sideslip, and TAS. MSFS derives IAS, Mach, angle of attack, and sideslip for its native instruments from the aircraft/model state. Quaternion attitude remains authoritative and Euler angles are derived only while constructing an MSFS frame.

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
