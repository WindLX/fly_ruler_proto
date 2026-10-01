# fly_ruler_proto

FlyRuler 的 protobuf/UDP 协议与数据内核，为飞行仿真提供状态更新、遥测流、时间序列存储与回放。协议 schema、版本号与生成代码集中在 `core/`，各语言绑定只做类型映射与生命周期封装。

## 目录结构

- `core/` —— Rust crate `fly_ruler_proto_core`。`core/proto/fly_ruler.proto` 是 wire schema 唯一事实源，另含 UDP 传输、时间序列存储、回放与管理 API。
- `server/` —— 独立管理守护进程 `fly_ruler_proto_server`（二进制 `fly-ruler-server`），同时提供 UDP 与 HTTP/WebSocket 管理接口。
- `bindings/python/` —— PyO3 绑定 `fly_ruler_proto_python` 与 Python 高层封装。
- `bindings/godot/` —— Godot 4 GDExtension（Rust `godot` crate，当前仅 Linux x86_64）。
- `bindings/msfs/` —— MSFS 2024 SimConnect 桥接，Windows/Proton 目标。
- `web/` —— Vue 3 + Vite 管理控制台。

详细用法见主仓[用户手册](../../docs/guide/components/proto.md)与[实现说明](../../docs/dev/components/proto.md)。

## 安装与构建

- Rust：`cargo build --workspace`，或 `just build-rust`。
- Python：`just setup-python` 同步依赖，`just build-python-dev` 本地安装扩展。
- Web：`just setup-web` 安装依赖，`just build-web` 产出 `web/dist`。
- MSFS 与 Godot 需要显式目标工具链，见 `just --list`。

## 测试

- `just check` / `just test` —— 全部语言面的检查与测试。
- `just check-rust|python|web`、`just test-rust|python|web` —— 分层执行。
- `just pre-commit` —— 格式、检查与测试的完整门。
- 具体命令以 `just --list` 为准。
