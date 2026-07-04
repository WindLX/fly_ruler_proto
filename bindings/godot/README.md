# Fly Ruler Protocol Godot Bindings（中文文档）

本目录提供 `fly_ruler_proto_core` 的 **Godot 4.2+** 绑定，基于 [gdext](https://github.com/godot-rust/gdext)（Rust GDExtension）实现。它面向飞行模拟的接收/可视化端，将 Rust 内核封装为 Godot 可直接调用的 `FlyRulerServer` 类。

## 1. 定位与架构

```text
Godot 引擎（GDScript）
    ↓
FlyRulerServer（GDExtension，Rust）
    ↓
KernelRuntime（core::kernel）
    ↓
TimeSeriesStore（core::store）  ←  UDP Server（core::transport）
```

- **Python 端**负责生成飞行器数据并发送。
- **Godot 端**负责接收 UDP 报文、维护会话、存储时序数据，并通过 `Dictionary` / `Array` 将状态暴露给 GDScript，供场景节点驱动 3D 模型。

## 2. 项目布局

```text
bindings/godot/
├── Cargo.toml
├── README.md                       # 本文件
├── src/
│   └── lib.rs                      # GDExtension 入口 + FlyRulerServer
├── scripts/
│   └── install_addon.sh            # Linux 一键安装脚本
└── templates/
    ├── fly_ruler_proto_godot.gdextension  # 扩展配置文件
    ├── FlyRulerDemo.gd                    # 示例 GDScript
    └── ADDON_LAYOUT.md                    # 推荐目录结构说明
```

## 3. 构建

### 3.1 从工作区根目录构建

```bash
# Debug
cargo build -p fly_ruler_proto_godot

# Release
cargo build -p fly_ruler_proto_godot --release
```

### 3.2 输出文件

| 平台 | Debug | Release |
|------|-------|---------|
| Linux x86_64 | `target/debug/libfly_ruler_proto_godot.so` | `target/release/libfly_ruler_proto_godot.so` |
| Windows x86_64 | `target/debug/fly_ruler_proto_godot.dll` | `target/release/fly_ruler_proto_godot.dll` |
| macOS | `target/debug/libfly_ruler_proto_godot.dylib` | `target/release/libfly_ruler_proto_godot.dylib` |

## 4. 安装到 Godot 项目

### 4.1 一键安装（Linux）

```bash
./bindings/godot/scripts/install_addon.sh /path/to/your_godot_project debug
```

Release 版本：

```bash
./bindings/godot/scripts/install_addon.sh /path/to/your_godot_project release
```

该脚本会自动：

- 编译 Rust 动态库
- 在目标 Godot 项目中创建 `res://addons/fly_ruler_proto/` 目录
- 复制 `.so`、`.gdextension` 与示例脚本 `FlyRulerDemo.gd`

### 4.2 手动安装

1. 在 Godot 项目中创建目录：

```text
res://addons/fly_ruler_proto/
```

2. 将对应平台的动态库复制到该目录。

3. 复制 `bindings/godot/templates/fly_ruler_proto_godot.gdextension` 到：

```text
res://addons/fly_ruler_proto/fly_ruler_proto_godot.gdextension
```

`.gdextension` 内容示例：

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = "4.2"

[libraries]
linux.debug.x86_64   = "res://addons/fly_ruler_proto/libfly_ruler_proto_godot.so"
linux.release.x86_64 = "res://addons/fly_ruler_proto/libfly_ruler_proto_godot.so"
windows.debug.x86_64   = "res://addons/fly_ruler_proto/fly_ruler_proto_godot.dll"
windows.release.x86_64 = "res://addons/fly_ruler_proto/fly_ruler_proto_godot.dll"
macos.debug   = "res://addons/fly_ruler_proto/libfly_ruler_proto_godot.dylib"
macos.release = "res://addons/fly_ruler_proto/libfly_ruler_proto_godot.dylib"
```

4. 重启 Godot 编辑器，确认 `.gdextension` 已被加载。

5. 可参考 `bindings/godot/templates/ADDON_LAYOUT.md` 中的推荐布局。

## 5. GDScript API 参考

暴露类名：`FlyRulerServer`（继承 `RefCounted`）。

### 5.1 服务器控制

#### `start_server(addr: String) -> bool`

启动 UDP 服务器。

```gdscript
var server := FlyRulerServer.new()
var ok := server.start_server("127.0.0.1:8080")
if not ok:
    push_error("Failed to start FlyRulerServer")
```

#### `stop_server() -> void`

停止 UDP 服务器。

```gdscript
server.stop_server()
```

#### `is_running() -> bool`

返回服务器是否正在监听。

```gdscript
if server.is_running():
    print("server is running")
```

#### `local_addr() -> String`

返回本地监听地址。未启动时返回空字符串。

```gdscript
print(server.local_addr())  # "127.0.0.1:8080"
```

### 5.2 会话信息

#### `active_sessions() -> Array[Dictionary]`

返回当前活跃会话列表。每个 `Dictionary` 包含：

| 键 | 类型 | 说明 |
|----|------|------|
| `addr` | `String` | 客户端地址，例如 `"127.0.0.1:54321"` |
| `client_uuid_hex` | `String` | 客户端 UUID 十六进制字符串 |
| `last_seen_secs` | `float` | 最后一次收到消息的时间戳（秒） |

```gdscript
for session in server.active_sessions():
    print(session["addr"], session["client_uuid_hex"])
```

### 5.3 飞行器数据查询

#### `get_aircraft_ids() -> PackedStringArray`

返回所有已记录飞行器 ID。

```gdscript
var ids := server.get_aircraft_ids()
for id in ids:
    print(id)
```

#### `get_latest_state(aircraft_id: String) -> Dictionary`

返回指定飞行器的最新状态。未找到时返回空 `Dictionary`。

返回 `Dictionary` 的键：

| 键 | 类型 | 说明 |
|----|------|------|
| `position` | `Dictionary` | `{x: float, y: float, z: float}` |
| `velocity` | `Dictionary` | `{x: float, y: float, z: float}` |
| `attitude` | `Dictionary` | `{w: float, x: float, y: float, z: float}` |
| `angular_velocity` | `Dictionary` | `{x: float, y: float, z: float}` |
| `derived` | `Dictionary` | `{lat, lon, altitude, alpha, beta, tas, eas, gamma, chi, ias?, cas?, mach?}` |
| `control_surfaces` | `Dictionary` | 可选舵面角度与襟翼/扰流板比例 |
| `engines` | `Array[Dictionary]` | `{index, throttle_lever_ratio?}` |
| `timestamp_secs` | `float` | 状态时间戳 |

```gdscript
var state := server.get_latest_state(aircraft_id)
if not state.is_empty():
    var pos: Dictionary = state["position"]
    var ts: float = state["timestamp_secs"]
    print("pos=", pos, " ts=", ts)
```

#### `get_states_in_range(aircraft_id: String, start: float, end: float) -> Array[Dictionary]`

按时间范围查询状态历史。每个元素结构与 `get_latest_state()` 返回一致。

```gdscript
var states := server.get_states_in_range(aircraft_id, 100.0, 110.0)
for s in states:
    print(s["timestamp_secs"])
```

> 性能提示：范围查询可能返回大量数据，不建议每帧调用。

#### `get_events_in_range(aircraft_id: String, start: float, end: float) -> Array[Dictionary]`

按时间范围查询事件历史。每个 `Dictionary` 包含：

| 键 | 类型 | 说明 |
|----|------|------|
| `timestamp_secs` | `float` | 事件发生时间 |
| `event_type` | `String` | `"spawn"` / `"despawn"` / `"custom"` |
| `name` | `String` | `spawn` 或 `custom` 事件的名称 |
| `toml_config` | `String` | `spawn` 事件的 TOML 配置 |
| `reason` | `String` | `despawn` 事件的原因（可能不存在） |

```gdscript
var events := server.get_events_in_range(aircraft_id, 0.0, 9999.0)
for e in events:
    print(e["event_type"], e.get("name", ""))
```

### 5.4 持久化

#### `save_session(path: String) -> bool`

将当前内存中的会话保存到磁盘。成功返回 `true`。

```gdscript
var ok := server.save_session("user://sessions/latest")
```

保存格式（由 `core::store` 实现）：

- `meta.json`：元数据
- `states.parquet`：状态数据
- `events.parquet`：事件数据

#### `load_session(path: String) -> bool`

从磁盘加载会话。

```gdscript
var ok := server.load_session("user://sessions/latest")
```

#### `clear_session() -> void`

清空内存中的所有飞行器状态与事件。

```gdscript
server.clear_session()
```

## 6. 生命周期

推荐在 Godot 节点中按以下方式使用：

```gdscript
extends Node

var server: FlyRulerServer

func _ready() -> void:
    server = FlyRulerServer.new()
    var ok := server.start_server("127.0.0.1:8080")
    if not ok:
        push_error("Failed to start FlyRulerServer")
        return
    print("FlyRuler server listening on: ", server.local_addr())

func _process(_delta: float) -> void:
    if server == null or not server.is_running():
        return

    var ids := server.get_aircraft_ids()
    for id in ids:
        var state := server.get_latest_state(id)
        if state.is_empty():
            continue
        var pos: Dictionary = state["position"]
        # 在此更新 Node3D 的位置/旋转

func _exit_tree() -> void:
    if server != null:
        server.stop_server()
```

### 生命周期说明

1. `_ready()`：构造 `FlyRulerServer` 并启动 UDP 监听。
2. `_process(delta)`：每帧轮询飞行器最新状态，用于驱动 3D 模型。
3. `_exit_tree()`：场景退出时停止服务器，释放端口与后台任务。

> 当前绑定未暴露 Godot `Signal`。状态获取采用轮询模式；若需要事件驱动，可在 GDScript 层用 `Timer` 或自定义 `Signal` 包装。

## 7. 数据字典结构说明

### 7.1 `Vector3` → `Dictionary`

```gdscript
{
    "x": 0.0,
    "y": 0.0,
    "z": 0.0
}
```

### 7.2 `Quaternion` → `Dictionary`

```gdscript
{
    "w": 1.0,
    "x": 0.0,
    "y": 0.0,
    "z": 0.0
}
```

### 7.3 `DerivedState` → `Dictionary`

```gdscript
{
    "lat": 0.0,
    "lon": 0.0,
    "altitude": 0.0,
    "alpha": 0.0,
    "beta": 0.0,
    "tas": 0.0,
    "eas": 0.0,
    "gamma": 0.0,
    "chi": 0.0,
    "ias": 0.0,
    "cas": 0.0,
    "mach": 0.0
}
```

### 7.4 `AircraftState` → `Dictionary`

```gdscript
{
    "position": {"x": ..., "y": ..., "z": ...},
    "velocity": {"x": ..., "y": ..., "z": ...},
    "attitude": {"w": ..., "x": ..., "y": ..., "z": ...},
    "angular_velocity": {"x": ..., "y": ..., "z": ...},
    "derived": {"lat": ..., ...},
    "control_surfaces": {"elevator_rad": ..., ...},
    "engines": [{"index": 1, "throttle_lever_ratio": 0.7}],
    "timestamp_secs": 123.456
}
```

## 8. 与核心库的集成

### 8.1 架构层级

```text
Godot 引擎 (GDScript)
    ↓
FlyRulerServer (GDExtension, Rust)
    ↓ 调用 block_on
KernelRuntime::start_server / stop_server / save_session / load_session
    ↓
TimeSeriesStore (DashMap 内存时序存储)
    ↓
ServerRuntime / Server (UDP 接收/发送)
```

### 8.2 关键集成点

- **全局 Tokio Runtime**：`bindings/godot/src/lib.rs` 使用 `std::sync::OnceLock` 懒加载一个多线程 Tokio Runtime，因为 Godot 主线程是单线程的，而核心库是异步的。
- **`block_on` 同步化**：所有 GDScript 调用最终通过 `runtime.block_on(...)` 转换为同步调用。
- **类型转换**：`aircraft_state_to_dict` 等函数将 Rust protobuf 类型转换为 Godot `Dictionary`。

## 9. 示例脚本

`templates/FlyRulerDemo.gd` 提供了一份可直接参考的示例：

```gdscript
extends Node

var server: FlyRulerServer

func _ready() -> void:
    server = FlyRulerServer.new()
    var ok := server.start_server("127.0.0.1:8080")
    if not ok:
        push_error("Failed to start FlyRulerServer")
        return
    print("FlyRuler server listening on: ", server.local_addr())

func _process(_delta: float) -> void:
    if server == null or not server.is_running():
        return
    var ids: PackedStringArray = server.get_aircraft_ids()
    for id in ids:
        var state := server.get_latest_state(id)
        if state.is_empty():
            continue
        var pos: Dictionary = state.get("position", {})
        var ts: float = float(state.get("timestamp_secs", 0.0))
        # 可在此更新 Node3D 位置/旋转

func save_current_session() -> bool:
    return server.save_session("user://sessions/latest")

func load_saved_session() -> bool:
    return server.load_session("user://sessions/latest")

func _exit_tree() -> void:
    if server != null:
        server.stop_server()
```

## 10. 常见问题

### 10.1 Godot 无法识别扩展

- 确认 `.gdextension` 文件与动态库位于同一目录。
- 确认 `entry_symbol = "gdext_rust_init"`。
- 确认 Godot 版本 >= 4.2。
- 重启 Godot 编辑器。

### 10.2 跨平台支持

目前 `.gdextension` 配置了 Linux、Windows、macOS 三个平台。若需要导出到其他平台，需：

1. 在该平台交叉编译 Rust 动态库；
2. 在 `.gdextension` 中增加对应条目。

### 10.3 性能建议

- `get_aircraft_ids()` 与 `get_latest_state()` 是内存查询，开销很小，适合每帧调用。
- `get_states_in_range()` 与 `get_events_in_range()` 可能返回大量数据，建议仅在需要回放或分析时调用。
- 长时间运行后若内存持续增长，可定期调用 `save_session()` 后 `clear_session()`。

## 11. 构建与验证

```bash
# 从仓库根目录
cargo build -p fly_ruler_proto_godot

# 运行核心测试
cargo test -p fly_ruler_proto_core
```

> 本绑定目前以服务器端为主；Godot 端不直接暴露客户端 API。若需要在 Godot 中发送数据，可考虑通过 GDScript 的 UDP API 或扩展绑定层。

## 12. 相关文档

- 内核文档：`../../core/README.md`
- Python 绑定：`../python/README.md`
- 项目总览：`../../CLAUDE.md`
- Protobuf Schema：`../../proto/fly_ruler.proto`
- 推荐布局：`templates/ADDON_LAYOUT.md`
