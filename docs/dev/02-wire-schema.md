# Wire schema 与协议版本

`core/proto/fly_ruler.proto` 是 FlyRuler Proto 的 wire schema 唯一事实源，包名 `flyruler`（`core/proto/fly_ruler.proto:3`）。Rust 类型、各语言绑定与字段说明都以它为准，字段编号一旦发布不再复用。

## 生成链路

```text
core/proto/fly_ruler.proto
        |  prost-build + vendored protoc
        v
$OUT_DIR/flyruler.rs
        |  include!
        v
core/src/pb.rs  ->  fly_ruler_proto_core::pb
```

- `core/build.rs:8` 注册 `rerun-if-changed`，schema 变更触发重新生成。
- `core/build.rs:13-18` 解析 vendored protoc 并写入 `PROTOC`。
- `core/build.rs:20-22` 用 `prost_build::Config::new().compile_protos()` 生成 Rust 类型。
- `core/src/pb.rs:8` 用 `include!(concat!(env!("OUT_DIR"), "/flyruler.rs"))` 引入生成代码。

生成的 Rust 与绑定胶水都不可手改，字段改动只发生在 `.proto` 文件。

## 消息族

| 消息 | 行 | 用途 |
| --- | --- | --- |
| `Uuid` | 5 | 16 字节标识，`bytes value = 1` |
| `Vector3` | 9 | 三分量向量 |
| `Quaternion` | 15 | 姿态四元数 w/x/y/z |
| `DerivedState` | 22 | 气动与导航派生量，后七项为 `optional` |
| `ControlSurfaceState` | 42 | 控制面与增升装置，全 `optional` |
| `PropulsorKind` | 52 | 推进器类型枚举 |
| `PropulsorState` | 59 | 单个推进器状态，`index` 供消费方映射槽位 |
| `TelemetryValueType` | 71 | 遥测标量类型枚举 |
| `TelemetryField` | 78 | 单个遥测字段元数据 |
| `TelemetryStreamSchema` | 87 | 一条遥测流的声明 |
| `TelemetryValue` | 94 | 带 `oneof kind` 的标量值 |
| `TelemetryFrame` | 102 | 按流发送的一帧样本 |
| `AircraftState` | 108 | 位置、速度、姿态、角速度、派生量、控制面、加速度与推进器 |
| `AircraftSpawnInfo` | 121 | 名称、TOML、初始状态与遥测声明 |
| `DespawnInfo` | 128 | 可选下线原因 |
| `AircraftCommandInfo` | 132 | spawn/despawn/state_update/custom_event/telemetry_frame 的 `oneof` |
| `AircraftEvent` | 142 | 飞机标识加命令 |
| `Handshake`、`ClientRole` | 147、153 | 版本、client UUID 与角色 |
| `Heartbeat` | 159 | 序号与 client UUID |
| `RequestCommand` | 164 | 握手、心跳、飞机事件、游标订阅与确认的 `oneof` |
| 游标消息族 | 174-258 | 订阅、帧、事件批、重置与分片 |
| `Request`、`Response` | 260、292 | 请求与响应信封，各自带时间戳 |
| `ErrorCode` | 275 | 错误码枚举 |
| `Message` | 302 | 顶层 `oneof envelope`，含 request/response/server_push |

## 字段编号与兼容

proto3 下字段编号是线格式身份，删除的编号不能复用，新增字段只能追加新编号。`AircraftState` 保留了两个空洞：现有编号为 1、2、3、4、5、7、9、10，缺 6 与 8（`core/proto/fly_ruler.proto:108-119`）。

反序列化会忽略未知字段：`core/src/pb.rs:80-89` 用一段含未知编号 6、8 的旧报文解码，已知的位置字段照常读出，未知部分直接丢弃，测试入口在 `core/src/pb.rs:47`。

`optional` 关键字决定 presence：`DerivedState` 的 ias、cas、mach、ground_speed、vertical_speed、dynamic_pressure、normal_load_factor 与 `ControlSurfaceState` 的全部字段都是 `optional`（`core/proto/fly_ruler.proto:32-50`），缺省与显式置零可区分，消费方按 `Option` 处理。

## 版本语义

`PROTOCOL_VERSION` 定义在 `core/src/lib.rs:34`，当前为 0.4.0，由 `core/src/transport/client.rs:51` 随握手送出。服务端用严格相等判定：`core/src/transport/server.rs:446` 比较 `hs.version == PROTOCOL_VERSION`，不等时回 `ErrorCode::ProtocolVersionMismatch` 与消息 `protocol version mismatch`（`core/src/transport/server.rs:461-464`）。客户端收到错误响应后转成 `TransportError::HandshakeRejected`（`core/src/transport/client.rs:266`）。

版本变更流程：只在字段语义、单位或必填性改变时提升版本，纯追加可选字段不提升。改版本走 `just set-version X.Y.Z`，脚本 `scripts/update_version.py` 同时改写 `Cargo.toml`、`core/src/lib.rs` 与 `web/package.json`；`just check` 内含的三处一致性校验不一致即失败。

## 两段式遥测

遥测流先声明后发帧。

声明在第一段：`AircraftSpawnInfo.telemetry_schemas`（`core/proto/fly_ruler.proto:125`）随 spawn 注册，写入 `AircraftConfig.telemetry_schemas` 后不可变（`core/src/store.rs:41-42`）。注册时校验流 ID 非空且唯一、`nominal_rate_hz` 为正、字段 ID 非空且在流内唯一、`value_type` 不是 UNSPECIFIED（`core/src/store.rs:1072`），任一项失败整条 spawn 被丢弃并记 warn（`core/src/store.rs:365-368`）。

发帧在第二段：`TelemetryFrame` 经 `AircraftCommandInfo.telemetry_frame`（`core/proto/fly_ruler.proto:138`）发送，值只带 `oneof kind` 标量，字段含义由已注册的 schema 决定。写入前核对值个数与逐个类型（`core/src/store.rs:1112`）；流 ID 未注册、个数不符或类型不符的帧分别被拒绝（`core/src/store.rs:1119-1121`、`:1122-1129`、`:1147-1151`）。

## 新增字段或消息

1. 在 `core/proto/fly_ruler.proto` 里追加字段并选用未使用过的编号，可选语义加 `optional`。
2. 运行 `just build` 或 `just check`，由 prost-build 重新生成 `fly_ruler_proto_core::pb`。
3. 需要落库或查询时，在 `core/src/store.rs` 的追加与查询路径补处理。
4. 需要走 UDP 或管理接口时，分别在 `core/src/transport/` 与 `core/src/management/` 补分支。
5. 按 `core/src/pb.rs` 的往返与未知字段用例补测试。
6. 同步各语言绑定与对应用户章节；语义变化再用 `just set-version X.Y.Z` 提升 `PROTOCOL_VERSION`。

## 信封与往返

顶层 `Message` 是 `oneof envelope`，含 `request = 1`、`response = 2`、`server_push = 3`（`core/proto/fly_ruler.proto:302-307`）。

请求带 `id`、源时间线 `timestamp` 与 `command`（`core/proto/fly_ruler.proto:260-266`）；响应回显同一 `id`，成功时 `ResponseData.ok` 是 `bool ack` 或 spawn 返回的 `aircraft_spawned` UUID（`core/proto/fly_ruler.proto:268-273`），失败时 `ResponseError` 带错误码、消息与可选 `aircraft_id`（`core/proto/fly_ruler.proto:286-290`）。

时间戳是产出方定义的源时间线秒数，同一会话必须用一致基准（`core/proto/fly_ruler.proto:262-263`）。服务端只处理客户端发来的 `Request`，对端的 `Response` 与 `ServerPush` 一律忽略（`core/src/transport/server.rs:516-521`）。

## 枚举与未知取值

六个枚举的首值都是 `*_UNSPECIFIED = 0`。取值判定走生成代码的 `try_from`，失败即未知，例如握手角色在 `core/src/transport/server.rs:447`；未指定角色会被拒（`core/src/transport/server.rs:448-455`）。

`ErrorCode` 现有八个取值：UNSPECIFIED、INVALID_AIRCRAFT_ID、TOML_PARSE_ERROR、UNKNOWN_FIELD、PROTOCOL_VERSION_MISMATCH、INVALID_STATE、INVALID_TELEMETRY_SCHEMA、INVALID_TELEMETRY_FRAME（`core/proto/fly_ruler.proto:275-284`）。

## 相关页面

- [架构总览](/dev/components/proto/01-architecture)：core 分层与数据流
- [UDP 会话与可靠性](/dev/components/proto/03-udp-session)：握手如何携带版本、ACK 与重传
- [遥测与时间序列](/guide/components/proto/03-telemetry)：产出方如何声明流并发送样本
- [接口参考](/dev/components/proto/api)：绑定公开面与 core 模块
- [Python API 参考](/api/proto/)：协议类型签名
