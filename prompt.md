# Fly Ruler Protocol Kernel - Rust Implementation

## 角色定位
你是资深的 Rust 系统工程师，专注于航空航天仿真协议、高性能二进制序列化和异步网络通信。你需要构建一个零拷贝、类型安全、跨语言（Python/Godot）的协议内核库。

## 项目背景与架构
本项目是飞行视景系统的共享协议层，位于 Python SDK（发送端）和 Godot Server（接收端）之间。
- **协议格式**：Length-delimited Bincode（二进制，非 JSON）
- **传输层**：Tokio TCP
- **目标**：1000Hz+ 状态更新，微秒级序列化延迟

## 技术栈（严格约束）
- **序列化**：`bincode` (v1.3+) with `serde` (derive feature)
- **异步运行时**：`tokio` (full feature)
- **缓冲区**：`bytes` crate (用于 zero-copy)
- **帧处理**：`tokio-util` (codec module)
- **错误处理**：`thiserror` (库内), `anyhow` (可选，用于 bin)
- **日志**：`tracing`
- **Python 互操作**：`pyo3` (预留 feature gate，命名为 `python-bindings`)
- **标准**：Rust 2021 Edition, MSRV 1.75

## 协议规范详解

### 1. 帧格式（TCP Stream）
必须实现自定义 Codec，处理粘包：
```
[4 bytes: payload length (big endian u32)] 
[N bytes: bincode serialized payload]
```

### 2. 核心数据结构（bincode + serde）

#### 基础数学类型（ Aerospace primitives）
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Attitude {
    pub phi: f64,   // roll (rad)
    pub theta: f64, // pitch (rad)
    pub psi: f64,   // yaw (rad)
}
```

#### 基础航空状态（12 状态 + 派生）
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseAircraftState {
    // 位置 (NED 坐标系: North, East, Down)
    pub position: Vector3,      // x, y, z (meters)
    // 速度 (机体坐标系)
    pub velocity: Vector3,      // u, v, w (m/s)
    // 姿态
    pub attitude: Attitude,
    // 角速度
    pub angular_velocity: Vector3, // p, q, r (rad/s)
    
    // 派生状态（可选，根据 flags 判断是否有效）
    pub derived: Option<DerivedState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedState {
    pub lat: f64,        // 纬度 (deg)
    pub lon: f64,        // 经度 (deg)
    pub altitude: f64,   // 海拔 (m)
    pub alpha: f64,      // 迎角 (rad)
    pub beta: f64,       // 侧滑角 (rad)
    pub tas: f64,        // 真空速 (m/s)
    pub eas: f64,        // 等效空速 (m/s)
    pub gamma: f64,      // 航迹倾角 (rad)
    pub chi: f64,        // 航迹方位角 (rad)
}
```

#### 自定义字段系统（支持 TOML 动态 schema）
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    F64(f64),
    I64(i64),
    Bool(bool),
    String(String),
    // 用于二进制 blob（如压缩的额外数据）
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomField {
    pub field_id: String,  // 唯一标识符，对应 TOML field_id
    pub value: FieldValue,
}
```

### 3. 命令枚举（协议消息）
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    // 连接握手
    Handshake {
        version: String,      // "1.0.0"
        capabilities: Vec<String>,
    },
    
    // 飞机生命周期
    AircraftSpawn {
        aircraft_id: String,          // UUID 或 callsign
        display_name: String,
        model_key: String,            // Godot 模型资源标识
        toml_config_hash: String,     // TOML 配置的 SHA256（用于缓存）
        toml_config: String,          // TOML 文件内容（首次传输）
        initial_state: BaseAircraftState,
    },
    
    AircraftDespawn {
        aircraft_id: String,
        reason: Option<String>,
    },
    
    // 核心：状态更新（高频）
    StateUpdate {
        aircraft_id: String,
        timestamp: f64,               // Unix timestamp with microsecond precision
        state: BaseAircraftState,
        custom_fields: Vec<CustomField>, // 舵面、油门等自定义数据
    },
    
    // 场景控制
    SceneControl {
        action: SceneAction,
        target_aircraft_id: Option<String>,
    },
    
    // 心跳（双向）
    Heartbeat {
        seq_num: u64,
        timestamp: f64,
    },
    
    // 错误报告（Godot -> Python）
    ErrorReport {
        code: ErrorCode,
        message: String,
        aircraft_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SceneAction {
    Pause,
    Resume,
    Reset,
    ClearAll,
    SpeedUp(f64), // 时间加速因子
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ErrorCode {
    InvalidAircraftId,
    TomlParseError,
    UnknownField,
    ProtocolVersionMismatch,
    InvalidState,
}
```

## 模块架构要求

```
src/
├── lib.rs           # 暴露公共 API
├── protocol.rs      # 核心数据结构（Command, State, 等）
├── codec.rs         # LengthDelimitedCodec 实现
├── transport.rs     # TCP Client/Server 抽象
└── python.rs        # PyO3 绑定（feature-gated）
```

### 关键实现细节

#### 1. Codec 实现（tokio-util）
实现 `Encoder<Command>` 和 `Decoder<Item=Command>`：
- 使用 `bytes::BytesMut` 作为缓冲区
- 大端序 u32 长度前缀
- 使用 `bincode::serialize_into` 直接写入 buffer 避免中间 Vec

```rust
// 示例结构（你需要完善）
pub struct BincodeCodec {
    // 配置 bincode options（固定字节序等）
    config: bincode::config::Configuration,
}
```

#### 2. 传输层抽象
提供通用的 `async fn send(&mut self, cmd: Command)` 和 `async fn recv(&mut self) -> Result<Command>`，不依赖具体 TCP 实现，便于测试。

#### 3. 零拷贝优化
对于 `StateUpdate` 中的大批量数据，考虑使用 `Cow<'a, [CustomField]>` 或类似机制，但优先保证 API 易用性。

#### 4. 类型安全
- 使用 `NonZeroU64` 等类型优化 Option 的内存布局（如果适用）
- 为 Aircraft ID 定义新类型 `pub struct AircraftId(String)` 防止混淆

## 开发任务（按优先级）

### Phase 1: 核心协议
1. 定义 `protocol.rs` 中的所有结构体，确保实现 `Serialize + Deserialize`
2. 实现 `codec.rs` 中的 `BincodeCodec`，处理 Length-delimited 帧
3. 添加单元测试：验证 roundtrip（序列化->反序列化）正确性
4. 使用 `criterion` 做基准测试：测试 1000 次 StateUpdate 序列化延迟

### Phase 2: 网络传输
1. 在 `transport.rs` 实现 `Connection` struct，包装 `Framed<TcpStream, BincodeCodec>`
2. 实现 `Client` 和 `Server` 基础结构（Listener 抽象）
3. 添加心跳超时检测逻辑

### Phase 3: Python 集成（feature = "python-bindings"）
1. 使用 `pyo3` 暴露 `Command` 和 `BaseAircraftState` 给 Python
2. 实现 `PyClient` 类，提供 `async def send_update()` 接口
3. 处理 Python 的 `asyncio` 与 Rust `tokio` 的运行时集成

## 代码质量要求

- **错误处理**：所有 `Result` 必须使用 `thiserror` 定义具体错误类型，禁止裸 `Box<dyn Error>`
- **文档**：每个 pub item 必须有 doc comment（包括单位、坐标系说明）
- ** unsafe**：禁止使用 unsafe（除非性能测试证明必需，且需注释）
- **测试**：核心序列化逻辑覆盖率 >90%，包括边界值（f64 NaN/Inf 处理）
- **Bincode 配置**：明确使用 `bincode::config::standard().with_big_endian().with_fixed_int_encoding()` 确保跨平台字节序一致

## 坐标系与单位（关键注释）
在代码文档中必须明确标注：
- **位置**：NED 坐标系（North-East-Down，X-北, Y-东, Z-下），单位米
- **姿态**：弧度（rad），遵循 Tuthill 约定（ZYX 欧拉角）
- **角速度**：机体坐标系（p-roll rate, q-pitch rate, r-yaw rate），rad/s

## 启动命令
请初始化项目：
1. `cargo init --lib fly_ruler_proto`
2. 配置 `Cargo.toml` 添加所有依赖
3. 创建上述模块文件结构
4. 先实现 `protocol.rs` 中的基础结构体定义和单元测试

开始编码。
