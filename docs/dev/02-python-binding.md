# Python 绑定

Python 绑定分三层：Rust 扩展模块 `fly_ruler_proto_python._core`、纯 Python 封装 `fly_ruler_proto_python.client`、以及手写类型存根 `_core.pyi`。理解这三层的分界，就能判断一个新字段应该加在哪里、为什么公开类的构造体验和 Rust 侧不一样。

## 三层分工

扩展模块在 `bindings/python/src/lib.rs:36` 的 `#[pymodule] fn _core` 里注册全部公开对象：九个数据类加 `PyClient`（`bindings/python/src/lib.rs:38-49`）、函数 `get_protocol_version()`（`bindings/python/src/lib.rs:26`，模块中位于 `bindings/python/src/lib.rs:52`），以及直接引用 core 常量得到的 `PROTOCOL_VERSION`（`bindings/python/src/lib.rs:55`）。

纯 Python 层在 `bindings/python/src/fly_ruler_proto_python/client.py`：`create_aircraft_state()`（`bindings/python/src/fly_ruler_proto_python/client.py:35`）补齐关键字参数与默认值，`FlyRulerClient`（`bindings/python/src/fly_ruler_proto_python/client.py:76`）负责参数校验、上下文管理与属性封装。`__init__.py` 把两者合起来构成公开面：`__all__` 在 `bindings/python/src/fly_ruler_proto_python/__init__.py:40`，`PropulsorKind` 枚举在 `:24`，自检入口 `main()` 在 `:61`。

`_core.pyi` 是手写存根，不是生成物。它决定编辑器与类型检查器看到的签名，所以新增或改动扩展模块公开面时，`bindings/python/src/fly_ruler_proto_python/_core.pyi` 必须同步修改，否则类型检查与实际运行时会不一致。

## 数据类与 protobuf 的转换

`bindings/python/src/protocol.rs` 定义全部数据类，每个类都紧跟一个到 protobuf 类型的 `From` 实现，把 Python 对象转成 wire 类型：

- `Vector3` 在 `bindings/python/src/protocol.rs:8`，`From<PyVector3> for pb::Vector3` 在 `bindings/python/src/protocol.rs:19`。
- `Attitude` 在 `bindings/python/src/protocol.rs:55`，标记为 `frozen`，因为它是经过校验的刚体姿态；`From<PyAttitude> for pb::Quaternion` 在 `bindings/python/src/protocol.rs:59`。
- `DerivedState` 在 `bindings/python/src/protocol.rs:129`，转换在 `bindings/python/src/protocol.rs:167`。
- `ControlSurfaceState` 在 `bindings/python/src/protocol.rs:241`，转换在 `bindings/python/src/protocol.rs:254`。
- `PropulsorState` 在 `bindings/python/src/protocol.rs:302`，转换在 `bindings/python/src/protocol.rs:424`。
- `TelemetryValueType` 在 `bindings/python/src/protocol.rs:316`，`TelemetryField` 在 `bindings/python/src/protocol.rs:336`，`TelemetryStreamSchema` 在 `bindings/python/src/protocol.rs:384`。
- `AircraftState` 在 `bindings/python/src/protocol.rs:467`，转换在 `bindings/python/src/protocol.rs:497`。

数据类用 `#[pyclass(from_py_object, name = "...", get_all, set_all)]` 暴露字段，因此 `state.position` 这类访问是零拷贝字段读取；`Attitude` 只读，构造必须走校验入口：`from_quaternion()`（`bindings/python/src/protocol.rs:75`）、`from_rotation_matrix()`（`bindings/python/src/protocol.rs:86`）、`from_euler()`（`bindings/python/src/protocol.rs:97`），非法输入返回 `ValueError` 而不是静默归一化。

`AircraftState` 的构造参数全部可选（`bindings/python/src/protocol.rs:531` 起的 `#[pyo3(signature = ...)]`），缺省时用零向量与单位姿态填充（`bindings/python/src/protocol.rs:512` 的 `default_for_rust()`），`AircraftState.hover()` 是它的静态快捷入口（`bindings/python/src/protocol.rs:566`）。`bindings/python/src/fly_ruler_proto_python/client.py:35` 的 `create_aircraft_state()` 就是在这个构造函数外用元组参数再包一层。

## 客户端对象与生命周期

`PyClient` 在 `bindings/python/src/client.rs:49`，构造签名与 Python 侧一一对应（`bindings/python/src/client.rs:72`）：地址、飞机名、初始状态、TOML 配置文本、心跳周期、遥测流声明、生成时间戳。构造过程直接完成连接、握手与生成飞机：一旦失败就抛异常，不会留下半初始化对象。

构造阶段的校验发生在 Rust 侧：时间戳必须是有限值，心跳周期必须是有限正数（`bindings/python/src/client.rs:81-86`）；遥测流 `stream_id` 必须非空且唯一，`nominal_rate_hz` 为正有限值（`bindings/python/src/client.rs:94-110`）。Python 封装层在 `bindings/python/src/fly_ruler_proto_python/client.py:123-127` 先做同样的检查，以便在进入扩展模块前给出更靠近调用点的错误。

异步运行时由扩展模块自己持有：`static RUNTIME: OnceLock<Runtime>`（`bindings/python/src/client.rs:19`）配合 `get_runtime()`（`bindings/python/src/client.rs:21`），所有网络操作在对象方法内 `block_on`（`bindings/python/src/client.rs:129`），因此 Python 侧调用是同步阻塞的。

生命周期方法：`client_uuid`（`bindings/python/src/client.rs:167`）、`aircraft_uuid`（`bindings/python/src/client.rs:172`）、`update_state`（`bindings/python/src/client.rs:178`）、`create_event`（`bindings/python/src/client.rs:192`）、`publish_telemetry`（`bindings/python/src/client.rs:206`）、`despawn`（`bindings/python/src/client.rs:252`）、`close`（`bindings/python/src/client.rs:265`）。除 `close()` 外每个方法都先调用 `ensure_open()`（`bindings/python/src/client.rs:341`），对象关闭后继续调用会得到 `ConnectionError`；网络层失败也被统一转成 `ConnectionError`（`bindings/python/src/client.rs:258`）。

`close()` 是幂等的（`bindings/python/src/client.rs:266-268`），并且会尽力补发一次 despawn 再释放内部客户端；`__del__` 只是忽略错误地调一次 `close()`（`bindings/python/src/client.rs:298`）。Python 侧的 `FlyRulerClient` 用 `_closed` 标志管理上下文退出（`bindings/python/src/fly_ruler_proto_python/client.py:231-252`），保证 `with` 语句退出后不会再发心跳。

## 公开面与同步要求

公开面固定为 14 个名字（`bindings/python/src/fly_ruler_proto_python/__init__.py:40`）。改动它的完整清单是：

1. `bindings/python/src/protocol.rs` 里加类与 `From` 转换。
2. `bindings/python/src/lib.rs` 的 `#[pymodule]` 内注册新类。
3. `bindings/python/src/fly_ruler_proto_python/_core.pyi` 补签名与 docstring。
4. 需要 Python 化默认值时，在 `bindings/python/src/fly_ruler_proto_python/client.py` 补封装函数，并同步 `__all__`。

## 测试

Rust 侧转换有单元测试（`bindings/python/src/protocol.rs:578` 起）。Python 行为测试在 `bindings/python/tests/test_core.py`，覆盖协议版本、状态构造、遥测声明校验与客户端生命周期；它同时是公开面回归，改名或删除导出会直接失败。

可运行的示例在 `bindings/python/examples/`，从 `01_connect_and_push.py`、`02_control_msfs.py`、`03_events_and_telemetry.py`、`04_sessions_and_playback.py`、`05_multi_aircraft.py` 到 `06_ai_fleet_msfs.py` 由简到繁，清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [接口参考](api.md)
- [UDP 会话与可靠性](advanced/udp-session.md)
- `docs/guide/05-telemetry.md`
- `docs/guide/04-control.md`
