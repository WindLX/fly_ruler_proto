# FlyRuler MSFS 2024 Bridge

This crate builds a Windows sidecar that receives FlyRuler UDP state and drives
the current Microsoft Flight Simulator 2024 user aircraft through SimConnect.
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

## Run

1. Start MSFS 2024 and enter a Free Flight with Active Pause disabled.
2. Start the bridge in the MSFS Proton prefix:

   ```bash
   uv tool install protontricks
   protontricks-launch --appid 2537590 \
     target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe
   ```

   `protontricks-launch` can emit a missing-`winetricks` warning when only its
   launcher functionality is used. This is non-fatal if the bridge proceeds to
   print `SimConnect connected`.

3. In another terminal, install the Python binding and start the demo:

   ```bash
   just develop
   cd bindings/python
   uv run python examples/demo_msfs_client.py
   ```

The bridge freezes latitude/longitude, altitude, and attitude only after the first valid FlyRuler state. A stale stream holds the final pose. Despawn, Ctrl-C, or normal shutdown restores the freeze state observed at startup.

Useful options:

```text
--listen 127.0.0.1:8080
--aircraft-id <32-character FlyRuler UUID>
--tick-hz 240
--stale-timeout-ms 500
--http-listen 127.0.0.1:8081
--data-root ./sessions
--ws-hz 30
--cors-origin http://localhost:5173
--no-http
```

Unless `--no-http` is supplied, the bridge embeds the same management service as `fly-ruler-server`. REST playback commands immediately affect SimConnect:
Live follows the newest sample, paused replay writes a seek exactly once, and playing replay advances using previous-value hold. A seek, load, clear, or other playback revision forces an explicit SimConnect refresh even when the selected sample has the same timestamp.

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

# TODO

- [ ] 引入 tracer log 和 python 还有 core 保持一致
- [ ] 从 TOML 配置文件传入参数