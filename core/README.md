# Fly Ruler Protocol Kernel

`fly_ruler_proto_core` 是 Fly Ruler 协议的内核实现，面向航空航天飞行模拟场景，提供高性能的二进制协议序列化、UDP 网络传输、时序数据存储、HTTP/WebSocket 管理接口与全局回放时间线。

它位于 **Python SDK（发送端）** 与 **Godot Server（接收/可视化端）** 之间，支持每秒 1000Hz 以上的飞行器状态更新，并将延迟控制在微秒级序列化开销内。

## 1. 协议概述

### 1.1 核心设计

| 项目 | 说明 |
|------|------|
| 应用协议 | protobuf 3 |
| 传输层 | UDP（Tokio `UdpSocket`） |
| 会话管理 | 应用层会话，基于 `client_uuid` + 握手/心跳维护 |
| 序列化 | `prost` 生成 Rust 类型，运行时编码/解码 |
| 协议版本 | `core/src/lib.rs` 中 `PROTOCOL_VERSION = "1.0.0"` |

### 1.2 消息帧格式

当前 UDP 实现中，每条消息直接编码为完整的 protobuf 二进制报文发送，不附加长度前缀：

```rust
let payload = prost::Message::encode_to_vec(&msg);
socket.send(&payload).await?;
```

接收端将 UDP 报文整体解码为 `pb::Message`：

```rust
let msg = prost::Message::decode(&buf[..n])?;
```

> 注：项目历史文档中提到过面向 TCP 的 `[4-byte big-endian u32 length] + [N-byte protobuf]` 帧格式；当前 UDP 路径未使用该格式。若未来需要 TCP 传输，可在 `codec` 模块恢复长度前缀编解码。

## 2. 工作区与目录结构

```text
fly_ruler_proto/
├── Cargo.toml                  # 工作区定义
├── justfile                    # 统一构建/测试入口
├── proto/
│   └── fly_ruler.proto         # Protobuf schema（唯一来源）
├── core/
│   ├── Cargo.toml
│   ├── build.rs                # 编译期 prost 代码生成
│   └── src/
│       ├── lib.rs              # 公共 API 与 PROTOCOL_VERSION
│       ├── pb.rs               # 生成的 protobuf 类型导出
│       ├── transport.rs        # 传输层错误与工具
│       ├── transport/
│       │   ├── client.rs       # UDP 客户端 / AircraftClient
│       │   └── server.rs       # UDP 服务端 / ServerRuntime
│       ├── kernel.rs           # KernelRuntime 运行时编排
│       ├── store.rs            # TimeSeriesStore 时序存储
│       ├── playback.rs         # Live/Replay 全局播放状态机
│       ├── management/         # Axum 管理服务、Series、Workspace 与持久化
│       │   ├── mod.rs          # runtime/router/CORS/static hosting 与 handlers
│       │   ├── gate.rs         # ingestion gate
│       │   ├── series.rs       # 字段目录、历史曲线查询与 LTTB
│       │   └── workspace.rs    # 全局 Web 工作区原子持久化
│       ├── config.rs           # 运行时配置
│       ├── logging.rs          # tracing 日志初始化
│       └── utils.rs            # 内部辅助函数（uuid_to_hex, now_secs）
└── core/tests/
    └── integration_core_flow.rs
```

## 3. Protobuf 协议

Schema 源文件：`proto/fly_ruler.proto`。`core/proto/fly_ruler.proto` 是随
crate 发布的镜像；`build.rs` 在工作区构建时会校验两者完全一致。

### 3.1 基础几何/状态消息

```protobuf
message Vector3 {
  double x = 1;
  double y = 2;
  double z = 3;
}

message Quaternion {
  double w = 1;  // 实部
  double x = 2;
  double y = 3;
  double z = 4;
}

message DerivedState {
  double lat      = 1;  // 纬度
  double lon      = 2;  // 经度
  double altitude = 3;  // 高度
  double alpha    = 4;  // 迎角
  double beta     = 5;  // 侧滑角
  double tas      = 6;  // 真空速
  double eas      = 7;  // 当量空速
  double gamma    = 8;  // 航迹倾斜角
  double chi      = 9;  // 航迹方位角
  optional double ias  = 10; // 指示空速
  optional double cas  = 11; // 校准空速
  optional double mach = 12; // 马赫数
}

message ControlSurfaceState {
  optional double aileron_left_rad  = 1;
  optional double aileron_right_rad = 2;
  optional double elevator_rad      = 3;
  optional double rudder_rad        = 4;
  optional double flaps_left_ratio  = 5;
  optional double flaps_right_ratio = 6;
  optional double spoilers_ratio    = 7;
}

message EngineState {
  uint32 index = 1; // 从 1 开始
  optional double throttle_lever_ratio = 2;
}

message AircraftState {
  Vector3              position         = 1;
  Vector3              velocity         = 2;
  Quaternion           attitude         = 3;
  Vector3              angular_velocity = 4;
  DerivedState         derived          = 5;
  repeated CustomField custom_fields    = 6;
  ControlSurfaceState  control_surfaces = 7;
  repeated EngineState engines          = 8;
}
```

### 3.2 飞行器事件

```protobuf
message AircraftSpawnInfo {
  string        name          = 1;
  string        toml_config   = 2;
  AircraftState initial_state = 3;
}

message DespawnInfo {
  optional string reason = 1;
}

message AircraftCommandInfo {
  oneof kind {
    AircraftSpawnInfo spawn        = 1;
    DespawnInfo       despawn      = 2;
    AircraftState     state_update = 3;
    string            custom_event = 4;
  }
}

message AircraftEvent {
  Uuid                aircraft_id = 1;
  AircraftCommandInfo info        = 2;
}
```

### 3.3 请求/响应消息

```protobuf
message Handshake {
  string version     = 1;
  Uuid   client_uuid = 2;
}

message Heartbeat {
  uint64 seq_num     = 1;
  Uuid   client_uuid = 2;
}

message RequestCommand {
  oneof kind {
    Handshake     handshake      = 1;
    Heartbeat     heartbeat      = 2;
    AircraftEvent aircraft_event = 3;
  }
}

message Request {
  Uuid           id        = 1;
  double         timestamp = 2;
  RequestCommand command   = 3;
}

message ResponseData {
  oneof kind {
    bool ack              = 1;
    Uuid aircraft_spawned = 2;
  }
}

enum ErrorCode {
  ERROR_CODE_UNSPECIFIED    = 0;
  INVALID_AIRCRAFT_ID       = 1;
  TOML_PARSE_ERROR          = 2;
  UNKNOWN_FIELD             = 3;
  PROTOCOL_VERSION_MISMATCH = 4;
  INVALID_STATE             = 5;
}

message ResponseError {
  ErrorCode code        = 1;
  string    message     = 2;
  Uuid      aircraft_id = 3;
}

message Response {
  Uuid   id        = 1;
  double timestamp = 2;
  oneof result {
    ResponseData  ok  = 3;
    ResponseError err = 4;
  }
}

message Message {
  oneof envelope {
    Request  request  = 1;
    Response response = 2;
  }
}
```

## 4. 核心模块说明

### 4.1 `lib.rs` — 公共 API 入口

```rust
pub const PROTOCOL_VERSION: &str = "1.0.0";
```

对外导出：

- 模块：`config`、`kernel`、`logging`、`pb`、`store`、`transport`
- 常用类型：`KernelRuntime`、`RuntimeError`、`TimeSeriesStore`、`AircraftClient`、`Client`、`Server`、`ServerRuntime`、各类 `Config` 等。

### 4.2 `pb.rs` + `build.rs` — 代码生成

`core/build.rs` 在工作区中从 `../proto/fly_ruler.proto` 生成 Rust 结构体；
从 crates.io 安装时则使用 crate 内的 `proto/fly_ruler.proto` 镜像。工作区
构建会先校验两个文件内容完全一致。生成结果在 `pb.rs` 中通过 `include!`
嵌入：

```rust
include!(concat!(env!("OUT_DIR"), "/flyruler.rs"));
```

> 不要手动编辑生成的 protobuf 代码。

### 4.3 `transport` 模块

#### 错误类型 `TransportError`

```rust
pub enum TransportError {
    Io(std::io::Error),
    Decode(prost::DecodeError),
    InvalidMessage(String),
    UnregisteredClient,
    ClientChannelClosed(&'static str),
}
```

#### `Client`（底层 UDP 客户端）

```rust
pub struct Client { ... }

impl Client {
    pub async fn connect(addr: &str, config: &LoggingConfig) -> Result<Self, TransportError>;
    pub async fn send(&mut self, msg: pb::Message) -> Result<(), TransportError>;
    pub async fn recv(&mut self) -> Result<Option<pb::Message>, TransportError>;
    pub async fn close(&mut self) -> Result<(), TransportError>;
}
```

#### `AircraftClient`（面向飞行器的高层客户端）

一个 `AircraftClient` 实例绑定一架飞行器的完整生命周期：

```rust
pub async fn connect(
    addr: &str,
    logging_config: &LoggingConfig,
    aircraft_name: String,
    initial_state: pb::AircraftState,
    toml_config: String,
    heartbeat_interval_secs: f64,
) -> Result<Self, TransportError>;

pub fn update_state(&self, state: pb::AircraftState, timestamp: Option<f64>) -> Result<(), TransportError>;
pub fn create_event(&self, event_name: String, timestamp: Option<f64>) -> Result<(), TransportError>;
pub async fn despawn(&mut self, reason: Option<String>, timestamp: Option<f64>) -> Result<(), TransportError>;
pub async fn close(&mut self) -> Result<(), TransportError>;
```

内部结构：

- `sender_handle`：后台 UDP 发送循环
- `operation_handle`：操作队列处理循环
- `heartbeat_handle`：定时心跳发送

#### `Server` 与 `ServerRuntime`

```rust
pub struct Server { ... }

impl Server {
    pub async fn bind(addr: &str) -> Result<Self, TransportError>;
    pub async fn recv_from(&self) -> Result<Option<(pb::Message, SocketAddr, Option<String>)>, TransportError>;
    pub async fn send_to(&self, msg: pb::Message, addr: SocketAddr) -> Result<(), TransportError>;
    pub fn set_session(&self, addr: SocketAddr, client_uuid: String);
    pub fn remove_session(&self, addr: SocketAddr);
    pub fn active_sessions(&self) -> Vec<Session>;
}

pub struct ServerRuntime { ... }

impl ServerRuntime {
    pub async fn start<F>(
        addr: &str,
        config: &TransportConfig,
        handler: F,
    ) -> Result<Self, TransportError>
    where
        F: Fn(pb::Message, SocketAddr) + Send + Sync + 'static;
    pub async fn stop(&self) -> Result<(), TransportError>;
    pub fn active_sessions(&self) -> Vec<Session>;
}
```

`ServerRuntime::start` 内部会：

1. `bind` UDP 端口；
2. `tokio::spawn` 接收循环；
3. 自动处理 `Handshake`（版本校验、注册会话、返回 ACK）；
4. 自动处理 `Heartbeat`（刷新会话、返回 ACK）；
5. 将 `AircraftEvent` 通过回调交给上层（如 `KernelRuntime`）。

### 4.4 `kernel.rs` — 运行时编排

```rust
pub struct KernelRuntime {
    store: Arc<TimeSeriesStore>,
    config: RuntimeConfig,
    playback: Arc<PlaybackController>,
    ingestion: Arc<IngestionGate>,
    udp_runtime: Option<ServerRuntime>,
    management_runtime: Option<ManagementServerRuntime>,
}

pub enum RuntimeError {
    Transport(TransportError),
    Store(StoreError),
    Management(ManagementError),
}
```

主要 API：

```rust
impl KernelRuntime {
    pub fn new(store: Arc<TimeSeriesStore>) -> Self;
    pub fn with_config(store: Arc<TimeSeriesStore>, config: RuntimeConfig) -> Self;

    pub async fn start_server(&mut self, addr: &str) -> Result<(), RuntimeError>;
    pub async fn stop_server(&mut self);
    pub async fn start_management_server(&mut self, addr: &str) -> Result<(), RuntimeError>;
    pub async fn stop_management_server(&mut self);

    pub async fn active_sessions(&self) -> Vec<Session>;
    pub fn udp_local_addr(&self) -> Result<SocketAddr, RuntimeError>;
    pub fn management_local_addr(&self) -> Result<SocketAddr, RuntimeError>;
    pub fn playback(&self) -> Arc<PlaybackController>;

    pub fn save_session(&self, path: &Path) -> Result<(), RuntimeError>;
    pub fn load_session(&self, path: &Path) -> Result<(), RuntimeError>;
    pub fn clear_session(&self);
}
```

`KernelRuntime` 将 UDP 服务端与 `TimeSeriesStore` 连接起来：收到事件后自动写入内存时序库，并支持会话查询与磁盘持久化。

### 4.5 `store.rs` — 时序存储

核心类型：

```rust
pub type AircraftId = String;

pub struct AircraftConfig {
    pub name: String,
    pub toml_config: String,
}

pub enum Event {
    Spawn(pb::AircraftSpawnInfo),
    Despawn(pb::DespawnInfo),
    Custom(String),
}

pub struct TimestampedState {
    pub timestamp_secs: f64,
    pub state: pb::AircraftState,
}

pub struct TimestampedEvent {
    pub timestamp_secs: f64,
    pub event: Event,
}

pub struct AircraftTimeSeries {
    pub states: Vec<TimestampedState>,
    pub events: Vec<TimestampedEvent>,
    pub config: Option<AircraftConfig>,
}

pub struct TimeSeriesStore {
    data: DashMap<AircraftId, AircraftTimeSeries>,
}
```

主要 API：

```rust
impl TimeSeriesStore {
    pub fn new() -> Self;

    pub fn append_state(&self, id: AircraftId, timestamp_secs: f64, state: pb::AircraftState);
    pub fn append_event(&self, id: AircraftId, timestamp_secs: f64, event: Event);
    pub fn append_message(&self, msg: pb::AircraftEvent);
    pub fn append_message_with_config(&self, msg: pb::AircraftEvent, config: Option<AircraftConfig>);

    pub fn get_latest(&self, id: &AircraftId) -> Option<TimestampedState>;
    pub fn get_state_at_or_before(&self, id: &AircraftId, at: f64) -> Option<TimestampedState>;
    pub fn is_spawned_at(&self, id: &AircraftId, at: f64) -> bool;
    pub fn get_states_page(&self, id: &AircraftId, start: f64, end: f64, offset: usize, limit: usize) -> Option<StorePage<TimestampedState>>;
    pub fn get_events_page(&self, id: &AircraftId, start: f64, end: f64, offset: usize, limit: usize) -> Option<StorePage<TimestampedEvent>>;
    pub fn get_states_range(&self, id: &AircraftId, start: f64, end: f64) -> Option<Vec<TimestampedState>>;
    pub fn get_events_range(&self, id: &AircraftId, start: f64, end: f64) -> Option<Vec<TimestampedEvent>>;
    pub fn get_aircraft_ids(&self) -> Vec<AircraftId>;
    pub fn clear(&self);

    pub fn save_to_disk(&self, path: &Path) -> Result<(), StoreError>;
    pub fn load_from_disk(&self, path: &Path) -> Result<(), StoreError>;
}
```

持久化格式：

- `meta.json`：版本、飞行器列表、时间范围、计数等元数据
- `states.parquet`：Arrow/Parquet 二进制（含 aircraft_id、timestamp、protobuf payload）
- `events.parquet`：Arrow/Parquet 二进制（含 aircraft_id、timestamp、event_type、payload）

### 4.6 `config.rs` — 运行时配置

```rust
pub struct TransportConfig {
    pub heartbeat_interval_secs: u64,  // 默认 5
    pub heartbeat_timeout_secs: u64,   // 默认 15
}

pub struct StoreConfig;

pub struct ManagementConfig {
    pub data_root: PathBuf,             // 默认 ./sessions
    pub web_root: Option<PathBuf>,       // 默认 ./web/dist
    pub public_api_base_url: Option<String>,
    pub public_websocket_url: Option<String>,
    pub websocket_hz: f64,              // 默认 30 Hz
    pub cors_origins: Vec<String>,
}

pub struct ReplayConfig {
    pub default_speed: f64,             // 默认 1.0
    pub min_speed: f64,                 // 默认 0.1
    pub max_speed: f64,                 // 默认 16.0
}

pub struct LoggingConfig {
    pub level: String,                 // 默认 "warn"
    pub file_path: Option<String>,
}

pub struct RuntimeConfig {
    pub transport: TransportConfig,
    pub store: StoreConfig,
    pub management: ManagementConfig,
    pub replay: ReplayConfig,
    pub logging: LoggingConfig,
}
```

生产前端不会嵌入 Rust binary。Release CI 只构建一次 `web/dist`，随后把
同一份产物放入 Linux server 和 MSFS 发布包。进程从解压目录启动时，
默认 `web_root` 可直接托管控制台；完整目录契约见
[`../RELEASING.md`](../RELEASING.md)。

### 4.7 HTTP、WebSocket 与回放

管理服务只允许绑定 loopback 地址。默认地址为 UDP
`127.0.0.1:18002`、HTTP/WS `127.0.0.1:18003`，独立进程可通过
`cargo run -p fly_ruler_proto_server` 启动。

回放是所有飞行器共享的全局时间线：

- `live`：每架飞机解析为最新状态；
- `replay_paused`：固定在游标处，使用前值保持；
- `replay_playing`：按 `0.1..=16.0` 正向推进，到末尾自动暂停。

UDP 在回放期间继续写入 Store。每次控制命令、load 或 clear 都递增
`revision`，渲染端可据此强制显式刷新。

REST 根路径为 `/api/v1`：

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/health`, `/status`, `/aircraft` | 健康、统计、飞机列表 |
| GET | `/aircraft/{id}/state?at=...` | 当前游标或指定时间的前值 |
| GET | `/aircraft/{id}/states`, `/events` | 时间范围分页查询 |
| GET | `/timeline/events` | 全部飞机事件的全局时间排序与分页 |
| GET/POST/PUT | `/playback...` | live、pause、play、seek、step、speed |
| POST | `/memory/clear` | 需提交 `{"confirm":true}` |
| GET | `/sessions` | 列出数据根目录内的快照 |
| POST | `/sessions/{name}/save`, `/load` | 异步保存/加载 |
| GET | `/operations/{id}` | 查询异步操作状态 |
| GET | `/aircraft/{id}/series/catalog` | 查询实际出现过的数值字段 |
| POST | `/series/query` | 批量历史曲线与完整区间统计 |
| GET/PUT | `/workspace` | 读取/保存跨浏览器工作区 |

`/api/v1/ws` 是严格只读的聚合快照流。它发送 `hello`、`snapshot`、
`operation_status`、`store_changed` 和 `workspace_changed`；控制命令必须走 REST。未指定飞机
筛选时最多发送 64 架，并用 `truncated` 标记截断。

`POST /playback/step` 接受
`{"unit":"sample|event","direction":"previous|next","count":1}`。sample
按全部飞机状态中的全局唯一时间戳跳转，event 按全部生命周期和 custom
事件的全局唯一时间戳跳转；`count` 范围为 `1..=100`，操作后进入暂停回放。

Series selector 使用带 `kind` 的 tagged JSON（`standard`、
`engine_throttle`、`custom`），避免 custom field ID 中的点号产生歧义。
一次查询最多 64 条曲线、每条 100–20,000 点；后端在完整区间统计后用
LTTB 降采样。Workspace 保存在 `data_root/.fly-ruler/workspace.json`，
每次更新递增 revision，并通知其他浏览器重新加载。

`ManagementConfig.web_root` 指向存在的目录时，服务会托管 Vue SPA。Rust
读取 `index.html` 模板并注入 `api_base_url` 与 `websocket_url`；默认使用
同源 `/api/v1` 和 `/api/v1/ws`，也可通过 public URL 配置覆盖。静态资源
和 SPA fallback 均由 Rust 提供，目录缺失时保持 API-only，且
`/api/v1/*` 始终返回 JSON 错误。

Web 曲线与时间轴使用全局数据起点作为相对零点，以避免 Unix 秒造成的坐标
压缩；REST、Store、Parquet 和 playback cursor 始终保留原始秒时间戳。

保存时只在克隆一致内存快照期间暂停 ingestion，落盘在后台继续；加载先
读入临时 Store，成功后才原子替换当前内存。维护窗口丢弃的 UDP 数量可从
`GET /status` 查看。

### 4.8 `logging.rs` — 日志初始化

- 使用 `tracing_subscriber` + `tracing_appender`
- 全局 `OnceLock` 保证仅初始化一次
- 默认过滤：`warn` 全局，并对核心模块设置 `info`/`warn` 级别
- `RUST_LOG` 环境变量优先

## 5. 错误处理

### 5.1 Rust 错误类型层次

```text
RuntimeError
├── TransportError
│   ├── Io
│   ├── Decode
│   ├── InvalidMessage
│   ├── UnregisteredClient
│   └── ClientChannelClosed
├── StoreError
│   ├── Io
│   ├── Json
│   ├── Parquet
│   ├── Arrow
│   ├── Decode
│   └── InvalidData
└── ManagementError
    ├── Io
    └── InvalidConfig
```

### 5.2 Protobuf 错误码

| 错误码 | 含义 |
|--------|------|
| `ERROR_CODE_UNSPECIFIED` | 未指定错误 |
| `INVALID_AIRCRAFT_ID` | 飞行器 ID 无效 |
| `TOML_PARSE_ERROR` | TOML 配置解析失败 |
| `UNKNOWN_FIELD` | 未知字段 |
| `PROTOCOL_VERSION_MISMATCH` | 协议版本不匹配 |
| `INVALID_STATE` | 状态数据无效 |

服务端在 `Handshake` 阶段校验客户端发送的版本号；若与 `PROTOCOL_VERSION` 不一致，则返回 `PROTOCOL_VERSION_MISMATCH`。

## 6. 生命周期

### 6.1 客户端生命周期（`AircraftClient`）

1. **连接阶段**：调用 `AircraftClient::connect()`
   - 创建底层 `Client`
   - 启动 sender / operation / heartbeat 三个后台任务
   - 自动发送 `Handshake`
   - 自动发送 `Spawn`（携带 `aircraft_name`、`toml_config`、`initial_state`）
2. **运行阶段**：
   - 用户调用 `update_state()` / `create_event()` → 通过 mpsc 操作队列异步发送
   - 心跳后台按 `heartbeat_interval_secs` 自动发送 `Heartbeat`
3. **关闭阶段**：调用 `close()`
   - 若尚未 despawn，自动发送 `Despawn(reason = "client_close")`
   - 停止心跳、终止 operation 任务
   - 等待所有后台任务结束

### 6.2 服务端生命周期（`ServerRuntime`）

1. **启动阶段**：`ServerRuntime::start()`
   - `Server::bind()` 绑定 UDP 端口
   - `tokio::spawn` 启动接收循环
2. **运行阶段**：
   - 收到 `Handshake` → 版本校验 → 注册会话 → 返回 ACK
   - 收到 `Heartbeat` → 刷新 `last_seen_secs` → 返回 ACK
   - 收到 `AircraftEvent` → 通过回调交给 `KernelRuntime` → 写入 `TimeSeriesStore`
   - 定期清理超过 `heartbeat_timeout_secs` 未活跃的会话
3. **停止阶段**：`stop()`
   - `CancellationToken::cancel()` 触发退出
   - 等待接收任务结束
   - 关闭 socket

## 7. 使用示例

### 7.1 最小服务端

```rust
use std::sync::Arc;
use fly_ruler_proto_core::{KernelRuntime, TimeSeriesStore, LoggingConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(TimeSeriesStore::new());
    let config = RuntimeConfig {
        logging: LoggingConfig::default(),
        ..Default::default()
    };
    let mut runtime = KernelRuntime::with_config(store, config);

    runtime.start_server("127.0.0.1:18002").await?;
    println!("server listening on {}", runtime.udp_local_addr()?);

    // 运行一段时间 ...
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;

    runtime.stop_server().await;
    Ok(())
}
```

### 7.2 最小客户端

```rust
use fly_ruler_proto_core::transport::AircraftClient;
use fly_ruler_proto_core::pb;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let initial_state = pb::AircraftState {
        // ... 初始化 position / velocity / attitude
        ..Default::default()
    };

    let mut client = AircraftClient::connect(
        "127.0.0.1:18002",
        &LoggingConfig::default(),
        "F-16".to_string(),
        initial_state,
        "[aircraft]\nname='F-16'".to_string(),
        1.0,
    ).await?;

    // 发送状态更新
    let state = pb::AircraftState { /* ... */ };
    client.update_state(state, None)?;

    // 发送自定义事件
    client.create_event("missile_launch".to_string(), None)?;

    // 结束生命周期
    client.despawn(Some("mission_complete".to_string()), None).await?;
    client.close().await?;
    Ok(())
}
```

### 7.3 查询与持久化

```rust
use fly_ruler_proto_core::{KernelRuntime, TimeSeriesStore, RuntimeConfig};
use std::sync::Arc;
use std::path::Path;

let store = Arc::new(TimeSeriesStore::new());

// 查询最新状态
if let Some(ts) = store.get_latest("some-aircraft-uuid") {
    println!("timestamp={}", ts.timestamp_secs);
}

// 保存到磁盘
store.save_to_disk(Path::new("session_backup"))?;

// 从磁盘恢复
store.load_from_disk(Path::new("session_backup"))?;
```

## 8. 构建与测试

推荐使用 `justfile` 提供的统一入口：

```bash
just check        # fmt + clippy
just test         # Rust + Python 测试
just test-rs      # cargo test --workspace
```

手动等价命令：

```bash
# 构建内核
cargo build -p fly_ruler_proto_core

# 运行内核单元测试与集成测试
cargo test -p fly_ruler_proto_core
```

集成测试位于 `core/tests/integration_core_flow.rs`，覆盖：

- UDP 运行时端到端：握手 → 生成 → 心跳 → 数据写入 → 会话可见
- `save_session` / `load_session` / `clear_session` 完整往返

## 9. 关键设计决策

1. **单一协议版本来源**：`PROTOCOL_VERSION` 仅定义在 `core/src/lib.rs`，所有绑定共享。
2. **显式持久化**：`save_to_disk` / `load_from_disk` 必须显式调用，不自动保存。
3. **DashMap 并发存储**：`TimeSeriesStore` 使用 `DashMap` 支持无锁并发读写。
4. **Parquet + Arrow 持久化**：状态/事件以 protobuf bytes 存入 Parquet，元数据存 JSON。
5. **应用层会话**：UDP 无连接，通过 `Handshake` / `Heartbeat` 中的 `client_uuid` 维护会话状态。
6. **ACK 策略**：`Handshake` 和 `Heartbeat` 成功均返回 ACK；版本不匹配返回错误响应。

## 10. 相关文档

- [Python 绑定](../bindings/python/README.md)
- [项目总览](../AGENT.md)
- [Protobuf Schema](../proto/fly_ruler.proto)
- [Crate 内 Schema 镜像](proto/fly_ruler.proto)

The management implementation is split into dedicated gate, series, workspace,
and HTTP runtime modules so persistence, replay control, and plotting queries
can evolve independently.
