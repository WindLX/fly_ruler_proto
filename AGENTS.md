# fly_ruler_proto 工作指南

## 项目用途

`fly_ruler_proto` 是 FlyRuler 的 protobuf/UDP 协议与数据内核，包含 Rust core、Python PyO3 binding、Godot binding 和 Web 管理界面。`core/proto/fly_ruler.proto` 是 wire schema 唯一事实源。

## 工具链与验证

- 查看命令：`just --list`；安装依赖：`just setup`；格式化：`just fmt`。
- 分层检查：`just check-rust`、`just check-python`、`just check-web`。
- 分层测试：`just test-rust`、`just test-python`、`just test-web`；完整：`just check`、`just test`。
- 构建：`just build`；交付前：`just pre-commit` 或 `just check-release`。
- MSFS/Windows 目标使用显式 `just build-msfs`、`just check-msfs`、`just package-msfs`，不属于默认本机验证。

## 本项目约束

- protobuf schema、协议版本与生成代码同步；生成的 prost/binding 代码不可手改。
- UDP session、ACK、heartbeat 与 best-effort 语义属于协议兼容面，变化需协议回归测试。
- PyO3 client/server 生命周期必须显式 close，并保持 context-manager 清理语义。
- core 不实现 UI replay、渲染插值或模型绑定；这些职责留给 consumer。
- Rust/Python/Godot/Web 暴露同一协议字段时同步更新所有绑定与文档。
- 不在内部 workspace crate 新增重复 AGENTS；根规则覆盖整个仓库。
- 编写 markdown 文档时，没有新段落时，不要因为行过宽而换行。
