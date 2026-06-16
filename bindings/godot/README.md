# fly_ruler_proto_godot

Godot 4 GDExtension wrapper for `fly_ruler_proto_core::KernelRuntime`.

## Exposed GDScript API

Class name: `FlyRulerServer` (RefCounted)

- `start_server(addr: String) -> bool`
- `stop_server() -> void`
- `is_running() -> bool`
- `local_addr() -> String`
- `active_sessions() -> Array[Dictionary]`
- `get_aircraft_ids() -> PackedStringArray`
- `get_latest_state(aircraft_id: String) -> Dictionary`
- `get_states_in_range(aircraft_id: String, start: float, end: float) -> Array[Dictionary]`
- `get_events_in_range(aircraft_id: String, start: float, end: float) -> Array[Dictionary]`
- `save_session(path: String) -> bool`
- `load_session(path: String) -> bool`
- `clear_session() -> void`

## Build

From workspace root:

```bash
cargo build -p fly_ruler_proto_godot
```

Output library path (Linux debug):

```bash
target/debug/libfly_ruler_proto_godot.so
```

## One-Command Install (Linux)

From workspace root:

```bash
./bindings/godot/scripts/install_addon.sh /path/to/your_godot_project debug
```

For release build:

```bash
./bindings/godot/scripts/install_addon.sh /path/to/your_godot_project release
```

## Godot Project Wiring

1. Create folder in your Godot project:

```text
res://addons/fly_ruler_proto/
```

2. Copy dynamic library into that folder.

3. Copy template file:

- `bindings/godot/templates/fly_ruler_proto_godot.gdextension`

into:

- `res://addons/fly_ruler_proto/fly_ruler_proto_godot.gdextension`

4. In Godot editor, ensure the `.gdextension` file is imported and available.

5. Use the demo script template:

- `bindings/godot/templates/FlyRulerDemo.gd`

to quickly validate start/stop/query flows.

You can also follow the standardized layout template:

- `bindings/godot/templates/ADDON_LAYOUT.md`

## Notes

- Runtime transport is UDP and delegates all protocol/session behavior to `core`.
- Methods returning dictionaries/arrays are designed for direct GDScript consumption.
- If your Godot version is not 4.2+, adjust `compatibility_minimum` in `.gdextension`.
