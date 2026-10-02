# MSFS 桥接实现

`bindings/msfs` 是 Windows 侧的 SimConnect 桥：它把 SimConnect 读到的飞行状态写进时间序列存储，再把存储里的状态按渲染周期写回模拟器。理解它的关键有三点：帧模型是纯数据、SimConnect 调用被编译门隔离、平滑逻辑与平台无关因而可以在 Linux 上测试。

## 模块划分与编译门

二进制入口是 `fly-ruler-msfs-bridge`（`bindings/msfs/Cargo.toml:8`）。`lib.rs` 承载全部与平台无关的逻辑，只有 `simconnect` 模块带 `#[cfg(windows)]`（`bindings/msfs/src/lib.rs:9-11`）；`main.rs` 同样分两个入口，Windows 下走桥接主循环，其他平台走提示路径（`bindings/msfs/src/main.rs:3-26`）。

因此非 Windows 机器上 `cargo test -p fly_ruler_proto_msfs` 依然能跑：帧映射、平滑、配置解析都在 lib 里。

## 帧模型

帧由几个纯数据结构组成，单位与符号约定都写在字段注释里：

- `MsfsPose`（`bindings/msfs/src/lib.rs:19`）：WGS-84 纬度、经度、海拔米，以及 MSFS 的俯仰、滚转、真航向（弧度）。
- `ControlSurfaces`（`bindings/msfs/src/lib.rs:36`）：左右副翼、升降舵、方向舵与左右襟翼、扰流板，全部是 `Option<f64>`，缺省表示该机型没有对应通道。
- `MsfsAirData`（`bindings/msfs/src/lib.rs:56`）：机体速度与角速度。
- `PropulsorThrottles`（`bindings/msfs/src/lib.rs:73`）与聚合结构 `MsfsFrame`（`bindings/msfs/src/lib.rs:80`）。
- 错误类型 `FrameError`（`bindings/msfs/src/lib.rs:93`）、可写控制面枚举 `Surface`（`bindings/msfs/src/lib.rs:116`）、起落架事件枚举 `GearCommand`（`bindings/msfs/src/lib.rs:135`）。

## 状态到帧的映射

`frame_from_state(&pb::AircraftState) -> Result<MsfsFrame, FrameError>`（`bindings/msfs/src/lib.rs:388`）是唯一入口：它把协议状态转成帧，无法表达的机型差异会以 `FrameError` 返回而不是猜测。可选字段缺失不会直接失败，`optional_field_warnings()`（`bindings/msfs/src/lib.rs:420`）会列出哪些字段被跳过，便于在日志里定位"飞机看起来正常但某个通道不动"的问题。

反向展开由 `frame_control_values()`（`bindings/msfs/src/lib.rs:576`）完成，它把一帧摊平成一串 `(Surface, f64)`，交给 SimConnect 逐通道写入。

机型选择在 `select_aircraft_at()`（`bindings/msfs/src/lib.rs:162`），起落架这类离散事件由 `GearEventTracker`（`bindings/msfs/src/lib.rs:198`）做边沿检测，避免把持续状态误报成事件。

## 平滑与实时缓冲

网络侧到达的状态与模拟器渲染周期不同步，中间层由 `smoothing.rs` 承担：

- `LiveFrameBuffer`（`bindings/msfs/src/smoothing.rs:151`）按渲染周期取帧，`LiveSmoothingConfig`（`bindings/msfs/src/smoothing.rs:60`）决定缓冲深度与超时。
- `SmoothingMode`（`bindings/msfs/src/smoothing.rs:10`）选择插值方式，`FrameSource`（`bindings/msfs/src/smoothing.rs:101`）区分实时帧与回放帧。
- `PushResult`（`bindings/msfs/src/smoothing.rs:88`）表达一次推入是否被节流或丢弃，`SmoothingStats`（`bindings/msfs/src/smoothing.rs:125`）累积统计用于排障。

`BridgeSession<S>`（`bindings/msfs/src/lib.rs:315`）把传输客户端、缓冲与写回串成会话，是主循环实际驱动的对象。

## SimConnect 层

`SimConnectClient`（`bindings/msfs/src/simconnect.rs:218`）封装 FFI：打开连接、注册数据定义、读取分派、写回数据与发送客户端事件。错误枚举 `SimConnectError`（`bindings/msfs/src/simconnect.rs:119`）与异常包装 `SimConnectException`（`bindings/msfs/src/simconnect.rs:127`）把 C 层返回码翻译成 Rust 错误。AI 飞机的创建请求由 `AiCreateRequest`（`bindings/msfs/src/simconnect.rs:148`）描述。

## 配置

命令行参数由 `Args`（`bindings/msfs/src/config.rs:16`）定义，`load()`（`bindings/msfs/src/config.rs:127`）负责合并命令行、TOML 文件与默认值。

`BridgeConfig`（`bindings/msfs/src/config.rs:111`）的关键项：UDP 监听地址 `listen`、机型过滤 `aircraft_id`、模拟步长 `tick_hz` 与渲染步长 `render_hz`、失联阈值 `stale_timeout_ms`、AI 飞机开关与上限（`enable_ai_aircraft`、`ai_aircraft_title`、`max_ai_aircraft`）、平滑配置 `smoothing`、内嵌管理接口 `management_enabled` 与 `http_listen`。

## 交叉编译与打包

`bindings/msfs/build.rs` 只在 Windows 目标下做事：先解析 SDK 根目录（`MSFS2024_SDK` 环境变量，缺省落回仓库内 `.msfs2024-sdk/MSFS 2024 SDK`，`bindings/msfs/build.rs:5-14`）；非 Windows 目标直接返回（`bindings/msfs/build.rs:21-23`）；随后断言 `SimConnect.h`、`SimConnect.lib`、`SimConnect.dll` 存在（`bindings/msfs/build.rs:33-39`）并检查头文件含必需的导出符号（`bindings/msfs/build.rs:42-53`）；最后设置链接参数并把 `SimConnect.dll` 复制到可执行文件旁边（`bindings/msfs/build.rs:55-66`）。

日常命令是 `just msfs build`（debug 交叉编译）、`just msfs build-release`、`just msfs check`（交叉 clippy）与 `just msfs package`（调用 `scripts/package_msfs_bundle.sh` 产出分发压缩包）。这些都不属于默认本机验证，需要显式调用。

## 测试

与平台无关的部分有单元测试：帧映射与事件在 `bindings/msfs/src/lib.rs:580` 起，平滑与缓冲在 `bindings/msfs/src/smoothing.rs:563` 起，配置解析在 `bindings/msfs/src/config.rs:310` 起。SimConnect 调用本身只能在 Windows 上验证，因此新增映射字段时要把"能离线断言的部分"尽量留在 `lib.rs` 与 `smoothing.rs`。

## 扩展点

新增一个写回通道需要四处对齐：`pb` 字段（`core/proto/fly_ruler.proto`）、`frame_from_state()` 的读取映射（`bindings/msfs/src/lib.rs:388`）、`frame_control_values()` 的展开（`bindings/msfs/src/lib.rs:576`）或 `GearEventTracker` 的事件边沿（`bindings/msfs/src/lib.rs:198`），以及 `BridgeConfig` 里是否需要新开关。只改前三处通常会导致接口能通、模拟器无反应。

配套的可运行示例是 `bindings/python/examples/02_control_msfs.py` 与 `bindings/python/examples/06_ai_fleet_msfs.py`，清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [架构总览](01-architecture.md)
- [wire schema 与兼容规则](advanced/wire-schema.md)
- [扩展点与验证](05-extending.md)
- [内核分层与并发](advanced/kernel-concurrency.md)
- `docs/guide/02-quickstart.md`
