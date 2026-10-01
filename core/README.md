# fly_ruler_proto_core

`fly_ruler_proto_core` 是 FlyRuler 的协议内核：定义 wire schema、UDP 传输运行时、时间序列存储、回放与管理 API，是 `server/` 与各语言绑定的共同底座。

## 事实源与入口

- `proto/fly_ruler.proto` —— 协议字段与 wire schema 的唯一事实源，由 `build.rs`（prost-build + 内置 protoc）生成 `pb` 模块；生成代码不可手改。
- `src/lib.rs` —— `PROTOCOL_VERSION`（当前 `0.4.0`）与公开模块声明；协议版本变化必须同步各绑定与测试。
- `src/transport/`、`src/store.rs`、`src/playback.rs`、`src/management/`、`src/kernel.rs` —— UDP 传输、时间序列存储、回放、管理接口与运行时编排。
- `src/attitude.rs`、`src/config.rs`、`src/cursor.rs`、`src/events.rs`、`src/logging.rs`、`src/utils.rs` —— 姿态、配置、游标、事件、日志与通用工具。
- `tests/integration_core_flow.rs` —— 端到端协议回归。

## 安装与测试

- 构建：`cargo build -p fly_ruler_proto_core`。
- 测试：`cargo test -p fly_ruler_proto_core`，或仓库入口 `just check-rust` / `just test-rust`。
