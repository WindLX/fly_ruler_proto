# Fly Ruler Protocol Python Bindings

本目录提供 `fly_ruler_proto_core` 的 Python 绑定，基于 **PyO3** 与 **maturin** 构建。它面向飞行模拟的数据发送端，将一架飞行器的完整生命周期封装为高层的 Python API，让 Python 脚本能够以最小网络细节向 Godot 或 Rust 服务端推送飞行器状态与事件。

Python wheels、Linux server 和自带 Web 控制台的 MSFS zip 由同一 tag workflow 发布，详见 [`../../RELEASING.md`](../../RELEASING.md)。

## 1. 定位与架构

```text
Python 脚本
    ↓
FlyRulerClient / PyClient（本绑定）
    ↓
fly_ruler_proto_core::transport::AircraftClient
    ↓
UDP 网络  →  Fly Ruler Server / MSFS / Godot
```

- `PyClient`：Rust 实现的 aircraft-bound 客户端（一个实例对应一架飞行器）。
- `FlyRulerClient`：Python 高层包装，提供上下文管理器与最佳实践。

发送到 `fly-ruler-server` 的状态可以在同仓库 Web 控制台中实时查看、绘制 历史曲线、按事件跳转和回放。显式 `timestamp` 仍使用秒；推荐 Unix wall time，前端会自动转换为相对仿真时间显示。

## 2. 环境准备

推荐工具链：

- [uv](https://docs.astral.sh/uv/)：Python 环境管理
- [maturin](https://www.maturin.rs/)：PyO3 扩展构建
- Rust toolchain（与仓库其他 crate 一致）

### 2.1 创建虚拟环境并安装构建工具

```bash
cd bindings/python

uv venv
source .venv/bin/activate  # Windows: .venv\Scripts\activate

uv pip install maturin pytest
```

### 2.2 构建并安装本地扩展

```bash
maturin develop
```

这会将 Rust 扩展编译为 `_core` 并安装到当前虚拟环境，Python 包可直接导入。

## 3. 数据类型

模块导出了与 protobuf schema 对齐的基础数据类。

### 3.1 `Vector3`

```python
from fly_ruler_proto_python import Vector3

v = Vector3(1.0, 2.0, 3.0)
print(v.x, v.y, v.z)

v.x = 9.0
zero = Vector3.zero()
```

| 属性 | 类型 | 说明 |
|------|------|------|
| `x` | `float` | X 分量 |
| `y` | `float` | Y 分量 |
| `z` | `float` | Z 分量 |

### 3.2 `Quaternion`

```python
from fly_ruler_proto_python import Quaternion

q = Quaternion(1.0, 0.0, 0.0, 0.0)
identity = Quaternion.identity()
```

| 属性 | 类型 | 说明 |
|------|------|------|
| `w` | `float` | 实部 |
| `x` | `float` | 虚部 i |
| `y` | `float` | 虚部 j |
| `z` | `float` | 虚部 k |

### 3.3 `DerivedState`

```python
from fly_ruler_proto_python import DerivedState

d = DerivedState(
    lat=37.7749,
    lon=-122.4194,
    altitude=500.0,
    alpha=0.05,
    beta=0.0,
    tas=50.0,
    eas=48.0,
    gamma=0.1,
    chi=0.2,
    ias=47.5,
    cas=47.5,
    mach=0.15,
)
```

| 属性 | 类型 | 说明 |
|------|------|------|
| `lat` | `float` | 纬度 |
| `lon` | `float` | 经度 |
| `altitude` | `float` | 高度 |
| `alpha` | `float` | 迎角 |
| `beta` | `float` | 侧滑角 |
| `tas` | `float` | 真空速 |
| `eas` | `float` | 当量空速 |
| `gamma` | `float` | 航迹倾斜角 |
| `chi` | `float` | 航迹方位角 |
| `ias` | `float \| None` | 指示空速 |
| `cas` | `float \| None` | 校准空速 |
| `mach` | `float \| None` | 马赫数 |

### 3.4 `AircraftState`

```python
from fly_ruler_proto_python import (
    AircraftState, ControlSurfaceState, DerivedState, EngineState,
    Quaternion, Vector3,
)

state = AircraftState(
    position=Vector3(100.0, 200.0, -300.0),
    velocity=Vector3(1.0, 2.0, 3.0),
    attitude=Quaternion(1.0, 0.0, 0.0, 0.0),
    angular_velocity=Vector3(0.1, 0.2, 0.3),
    derived=DerivedState(lat=30.0, lon=120.0, altitude=1000.0),
    control_surfaces=ControlSurfaceState(rudder_rad=0.1),
    engines=[EngineState(1, throttle_lever_ratio=0.7)],
    custom_fields={"flyruler.control.rudder_rad": 0.1},
)

# 悬停/默认状态
hover = AircraftState.hover()
```

| 属性 | 类型 | 说明 |
|------|------|------|
| `position` | `Vector3` | 位置 |
| `velocity` | `Vector3` | 速度 |
| `attitude` | `Quaternion` | 姿态四元数 |
| `angular_velocity` | `Vector3` | 角速度 |
| `derived` | `DerivedState \| None` | 派生气动/导航状态 |
| `control_surfaces` | `ControlSurfaceState \| None` | 标准舵面状态 |
| `engines` | `list[EngineState]` | 从 1 开始编号的逐发动机状态 |
| `custom_fields` | `dict[str, float \| int \| bool \| str \| bytes]` | protobuf 扩展字段 |

### 3.5 辅助函数 `create_aircraft_state`

```python
from fly_ruler_proto_python import create_aircraft_state

state = create_aircraft_state(
    position=(1.0, 2.0, 3.0),
    velocity=(4.0, 5.0, 6.0),
    attitude=(1.0, 0.1, 0.2, 0.3),
    angular_velocity=(0.4, 0.5, 0.6),
    derived=DerivedState(lat=30.0, lon=120.0, altitude=1000.0),
    control_surfaces=ControlSurfaceState(rudder_rad=0.1),
    engines=[EngineState(1, throttle_lever_ratio=0.7)],
    custom_fields={"flyruler.control.rudder_rad": 0.1},
)
```

提供元组形式的便捷构造，并可传入标准空气数据、舵面、逐发动机状态与
类型化 `custom_fields`。

## 4. `FlyRulerClient` — 高层客户端

### 4.1 构造参数

```python
class FlyRulerClient:
    def __init__(
        self,
        address: str,
        aircraft_name: str,
        initial_state: AircraftState | None = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
    ) -> None: ...
```

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `address` | `str` | 必填 | 服务端地址，例如 `"127.0.0.1:18002"` |
| `aircraft_name` | `str` | 必填 | 飞行器名称 |
| `initial_state` | `AircraftState \| None` | `None` | 初始状态，默认悬停 |
| `toml_config` | `str` | `""` | TOML 格式飞行器配置 |
| `heartbeat_interval_secs` | `float` | `1.0` | 心跳间隔（秒） |

构造函数会**自动完成**：连接 UDP、发送 Handshake、发送 Spawn 请求。

### 4.2 属性

```python
client.client_uuid     # str，客户端 UUID
client.aircraft_uuid   # str，飞行器 UUID
```

### 4.3 方法

#### `update_state(state, timestamp=None)`

```python
client.update_state(state)
client.update_state(state, timestamp=123.456)
```

向服务端发送飞行器状态更新。

#### `create_event(event_name, timestamp=None)`

```python
client.create_event("missile_launch")
client.create_event("missile_launch", timestamp=123.456)
```

发送自定义事件。

MSFS bridge 识别两个标准起落架事件：

```python
client.create_event("flyruler.control.gear_up")
client.create_event("flyruler.control.gear_down")
```

#### `despawn(reason=None, timestamp=None)`

```python
client.despawn(reason="mission_complete")
```

发送 Despawn 事件，标记该飞行器生命周期结束。

#### `close()`

```python
client.close()
```

- 若尚未 despawn，会自动发送 `Despawn(reason="client_close")`。
- 停止后台任务（sender / operation / heartbeat）。
- 关闭网络连接。

### 4.4 上下文管理器

推荐使用 `with` 语句，确保退出时自动关闭：

```python
from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state

with FlyRulerClient("127.0.0.1:18002", "F-16") as aircraft:
    aircraft.update_state(create_aircraft_state(position=(100.0, 0.0, -1000.0)))
    aircraft.create_event("missile_launch")
```

## 5. `PyClient` — 底层 Rust 客户端

`FlyRulerClient` 内部包装了 `PyClient`。若需要直接使用 Rust 层暴露的客户端，可从 `_core` 导入：

```python
from fly_ruler_proto_python._core import PyClient, AircraftState

client = PyClient(
    "127.0.0.1:18002",
    "F-16",
    AircraftState.hover(),
    "",
    1.0,
)
```

`PyClient` 提供的方法与 `FlyRulerClient` 一致：

- `client_uuid()`
- `aircraft_uuid()`
- `update_state(state, timestamp=None)`
- `create_event(event_name, timestamp=None)`
- `despawn(reason=None, timestamp=None)`
- `close()`

区别：

- `PyClient` 在析构时 `__del__` 会尝试自动调用 `close()`。
- `FlyRulerClient` 提供更符合 Python 习惯的高阶封装（上下文管理器、`create_aircraft_state` 集成等）。

## 6. MSFS 多机/AI 示例

一个 `FlyRulerClient` 实例对应一架 aircraft。你可以：

- 在同一个 Python 进程里创建多个 `FlyRulerClient`。
- 或者启动多个 Python client 进程，分别发送不同 aircraft。

MSFS bridge 会把一个 aircraft 映射到 user aircraft；当 bridge 使用
`--enable-ai-aircraft` 启动时，其余 spawned aircraft 会作为 AI 视觉飞机渲染。

先启动 bridge：

```bash
./fly-ruler-msfs-bridge.exe \
  --enable-ai-aircraft \
  --ai-aircraft-title "Rafale M" \
  --max-ai-aircraft 8
```

然后从一个进程发送三架飞机：

```bash
cd bindings/python
uv run python examples/demo_msfs_ai_client.py --aircraft-count 3
```

脚本会打印每架飞机的 `aircraft_uuid`。如果希望固定哪一架是 MSFS user aircraft，
把对应 UUID 传给 bridge：

```bash
./fly-ruler-msfs-bridge.exe \
  --aircraft-id <printed-aircraft-uuid-without-dashes> \
  --enable-ai-aircraft \
  --ai-aircraft-title "Rafale M"
```

也可以启动多个独立进程，每个进程只发送一架：

```bash
uv run python examples/demo_msfs_ai_client.py --aircraft-count 3 --aircraft-index 0
uv run python examples/demo_msfs_ai_client.py --aircraft-count 3 --aircraft-index 1
uv run python examples/demo_msfs_ai_client.py --aircraft-count 3 --aircraft-index 2
```

注意：`--ai-aircraft-title` 必须是 MSFS 里可用的 aircraft title；复杂第三方飞机在
AI 模式下可能不会完整驱动座舱/HUD/插件动画，建议先用 stock aircraft 验证多机外部视景。

## 7. 协议版本

```python
from fly_ruler_proto_python import PROTOCOL_VERSION, get_protocol_version

print(PROTOCOL_VERSION)           # "1.0.0"
print(get_protocol_version())     # "1.0.0"
```

版本号来自 `fly_ruler_proto_core::PROTOCOL_VERSION`，所有绑定共享同一来源。

## 8. 日志

Python 绑定在首次创建运行时（即构造 `PyClient` ）时，会初始化一次 `tracing` 订阅器。初始化是幂等的。

默认日志过滤：

```text
warn,
fly_ruler_proto_python.client=info,
fly_ruler_proto_python.server=info,
fly_ruler_proto_core.runtime=warn,
fly_ruler_proto_core.store=warn,
fly_ruler_proto_core.transport=warn
```

可通过 `RUST_LOG` 环境变量覆盖：

```bash
RUST_LOG=debug python your_script.py
```

## 9. 完整示例

```python
import time
from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state

SERVER = "127.0.0.1:18002"


def main():
    with FlyRulerClient(SERVER, "F-16", heartbeat_interval_secs=1.0) as aircraft:
        print(f"client_uuid={aircraft.client_uuid}")
        print(f"aircraft_uuid={aircraft.aircraft_uuid}")

        for i in range(100):
            state = create_aircraft_state(
                position=(float(i), 0.0, -1000.0),
                velocity=(10.0, 0.0, 0.0),
            )
            aircraft.update_state(state, timestamp=time.time())
            time.sleep(0.001)  # 1000Hz

        aircraft.create_event("waypoint_reached", timestamp=time.time())


if __name__ == "__main__":
    main()
```

## 10. 测试

```bash
cd bindings/python
source .venv/bin/activate

# Rust 侧单元测试
cargo test -p fly_ruler_proto_python

# Python 侧测试
pytest tests/
```

测试覆盖：

- 数据类构造与属性读写（`Vector3`、`Quaternion`、`DerivedState`、`AircraftState`）
- `create_aircraft_state` 辅助函数
- 协议版本一致性
- `FlyRulerClient` 包装器转发与上下文管理器行为

## 11. 错误处理

绑定层将 Rust 错误统一映射为 Python `ConnectionError`：

```python
try:
    client = FlyRulerClient("127.0.0.1:18002", "F-16")
except ConnectionError as e:
    print(f"连接失败: {e}")
```

常见失败原因：

- 网络不可达
- 服务端未启动
- 协议版本不匹配（此时服务端会返回错误响应，连接仍可能建立，但后续操作可能失败）

## 12. 构建配置速查

`pyproject.toml` 中的关键配置：

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "fly_ruler_proto_python"
version = "0.2.3"
requires-python = ">=3.10"
```

- 包名：`fly_ruler_proto_python`
- Rust crate：`bindings/python/Cargo.toml` 中的 `fly_ruler_proto_python`
- 入口模块：`src/lib.rs` 定义 `_core`，Python 包通过 `__init__.py` 重新导出。

## 13. 相关文档

- 内核文档：`../../core/README.md`
- Godot 绑定：`../godot/README.md`
- 项目总览：`../../CLAUDE.md`
- Protobuf Schema：`../../proto/fly_ruler.proto`
