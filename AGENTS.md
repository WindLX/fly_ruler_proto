# fly_ruler_proto

FlyRuler protobuf/UDP 协议与数据内核，含 Rust、Python、Godot 和 Web bindings。

## 事实源与入口

- **约定**：`core/proto/fly_ruler.proto` 是 wire schema 唯一事实源；命令以 `just --list` 为准。
- **约定**：分层检查/测试使用 `just check-rust|python|web`、`just test-rust|python|web`；完整门为 `just check`、`just test`、`just pre-commit`。
- **约定**：MSFS/Windows 使用显式 `just build-msfs`、`just check-msfs`、`just package-msfs`，不属于默认本机验证。
- `core/proto/fly_ruler.proto` —— 协议字段与 wire schema

## 本目录特有边界

- **必须**：protobuf schema、协议版本和生成代码同步；生成的 prost/binding 代码不可手改。
- **必须**：UDP session、ACK、heartbeat、best-effort 语义变化有协议回归测试。
- **必须**：PyO3 client/server 显式 close 并保持 context-manager 清理语义。
- **禁止**：core 实现 UI replay、渲染插值或模型绑定；这些职责属于 consumer。
- **必须**：Rust/Python/Godot/Web 的协议字段绑定与文档同步更新。
- **禁止**：在内部 workspace crate 新增重复 AGENTS；本文件已覆盖本仓。
