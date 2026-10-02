# 接口参考

这一页是索引：列出 Python 绑定的公开名字与 Rust 内核的公开模块，说明每个对象负责什么，并把签名细节交给自动生成的 API 参考。写代码前先看这里的组合方式，比逐个翻签名更快。

## Python 公开面

公开面固定为 14 个名字，`__all__` 在 `bindings/python/src/fly_ruler_proto_python/__init__.py:40`。

| 名字 | 用途 | 定义位置 |
| --- | --- | --- |
| `PROTOCOL_VERSION` | 协议语义版本字符串 | `bindings/python/src/lib.rs:55` |
| `get_protocol_version()` | 以函数形式返回同一版本号 | `bindings/python/src/lib.rs:26` |
| `Vector3` | 三分量向量，构造需要三个浮点 | `bindings/python/src/protocol.rs:8` |
| `Attitude` | 校验过的姿态，支持四元数、旋转矩阵与欧拉角互转 | `bindings/python/src/protocol.rs:55` |
| `DerivedState` | 气动与导航派生量 | `bindings/python/src/protocol.rs:129` |
| `ControlSurfaceState` | 控制面物理状态 | `bindings/python/src/protocol.rs:241` |
| `PropulsorState` | 单个推进器状态 | `bindings/python/src/protocol.rs:302` |
| `PropulsorKind` | 推进器类型枚举 | `bindings/python/src/fly_ruler_proto_python/__init__.py:24` |
| `TelemetryValueType` | 遥测标量类型 | `bindings/python/src/protocol.rs:316` |
| `TelemetryField` | 单个遥测字段的元数据 | `bindings/python/src/protocol.rs:336` |
| `TelemetryStreamSchema` | 一条遥测流的声明 | `bindings/python/src/protocol.rs:384` |
| `AircraftState` | 完整飞机状态，全部字段可选 | `bindings/python/src/protocol.rs:467` |
| `FlyRulerClient` | 绑定单架飞机生命周期的客户端 | `bindings/python/src/fly_ruler_proto_python/client.py:76` |
| `create_aircraft_state()` | 带默认值的状态构造助手 | `bindings/python/src/fly_ruler_proto_python/client.py:35` |

公开名字的构造与调用方式在 Rust 扩展模块里定义，纯 Python 层只负责补默认值与校验，分界见 [Python 绑定](02-python-binding.md)。

### 组合方式

```python
from fly_ruler_proto_python import (
    FlyRulerClient,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
    create_aircraft_state,
)

schema = TelemetryStreamSchema(
    stream_id="aero",
    fields=[
        TelemetryField(
            field_id="mach",
            label="Ma",
            unit="1",
            value_type=TelemetryValueType.F64,
        )
    ],
    nominal_rate_hz=10.0,
)

with FlyRulerClient("127.0.0.1:18002", "F-16", telemetry_schemas=[schema]) as aircraft:
    aircraft.update_state(create_aircraft_state(position=(100.0, 0.0, -1000.0)))
    aircraft.create_event("flyruler.control.gear_down")
```

事件名没有枚举约束，字符串原样通过网络发送；`flyruler.control.gear_up` 与 `flyruler.control.gear_down` 是协议保留名（`core/src/events.rs:4`、`core/src/events.rs:7`），模拟器侧消费者会识别它们。

### 读签名的边界

- 构造函数即完成连接、握手与生成飞机，失败直接抛异常，不存在"已创建但未连接"的实例（`bindings/python/src/fly_ruler_proto_python/client.py:79-97`）。
- `close()` 幂等，`with` 退出后会再调一次也安全；关闭后继续调用其他方法会得到 `ConnectionError`（`bindings/python/src/client.rs:341`）。
- 遥测流的 `stream_id` 必须非空且互不重复，`nominal_rate_hz` 若给出必须是有限正数（`bindings/python/src/client.rs:94-110`）。
- `Attitude` 只读，必须通过 `from_quaternion()`、`from_rotation_matrix()`、`from_euler()` 构造，非法输入抛 `ValueError` 而不是自动归一化（`bindings/python/src/protocol.rs:75-107`）。
- proto3 的可选字段在 Python 侧表现为 `None`，不是零值；只有需要区分"未提供"与"零"的字段才使用可选语义。
- 游标与回放订阅不在 Python 公开面内，需要通过管理 HTTP 接口调用，操作方式见 `docs/guide/06-sessions.md`。

完整签名与每个字段的单位说明见主仓生成的接口参考（`../docs/api/proto/`），改注释后在主仓用 `cd docs && just api` 重新生成。

## Rust 公开模块

内核 crate `fly_ruler_proto_core` 的公开模块在 `core/src/lib.rs:10-30` 声明：

| 模块         | 内容                                 | 定义位置             |
| ------------ | ------------------------------------ | -------------------- |
| `attitude`   | 姿态校验与旋转运算                   | `core/src/lib.rs:10` |
| `config`     | 运行、传输、存储、回放与管理配置类型 | `core/src/lib.rs:12` |
| `cursor`     | 游标对齐的快照组装与 UDP 流式类型    | `core/src/lib.rs:14` |
| `events`     | 协议消费者共享的自定义事件名         | `core/src/lib.rs:16` |
| `kernel`     | 内核编排与服务器生命周期             | `core/src/lib.rs:18` |
| `logging`    | tracing 订阅器初始化                 | `core/src/lib.rs:20` |
| `management` | HTTP/WebSocket 管理 API              | `core/src/lib.rs:22` |
| `pb`         | 由 schema 生成的 protobuf 类型       | `core/src/lib.rs:24` |
| `playback`   | 实时与回放共用的时间轴控制器         | `core/src/lib.rs:26` |
| `store`      | 时间序列存储与持久化                 | `core/src/lib.rs:28` |
| `transport`  | UDP 传输运行时                       | `core/src/lib.rs:30` |

常用类型通过 crate 根重导出，避免下游写深路径：`core/src/lib.rs:37-63` 导出 `Attitude`/`AttitudeError`、配置类型、游标客户端与运行时、`KernelRuntime`/`RuntimeError`、`init_logging`、管理与回放类型、`TimeSeriesStore` 与相关数据结构和传输类型。`PROTOCOL_VERSION` 是 crate 根常量（`core/src/lib.rs:34`）。

内核 crate 是唯一发布到 crates.io 的 Rust 包，版本化文档见 [docs.rs 上的 fly_ruler_proto_core](https://docs.rs/fly_ruler_proto_core)。

## 相关页面

- [Python 绑定](02-python-binding.md)
- [架构总览](01-architecture.md)
- [wire schema 与兼容规则](advanced/wire-schema.md)
- [内核分层与并发](advanced/kernel-concurrency.md)
- `docs/guide/01-install.md`
