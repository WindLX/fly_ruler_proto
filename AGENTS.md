# fly_ruler_proto

FlyRuler protobuf/UDP 协议与数据内核：Rust 内核与服务器、Python/Godot/MSFS 绑定、Vue 控制台，可独立发包。

## 事实源与入口

- **约定**：命令以 `just --list` 为准；安装 `just setup`，格式 `just fmt`，检查 `just check`，测试 `just test`，构建 `just build`，交付 `just pre-commit`。
- **约定**：MSFS 与 Windows 相关步骤不进入默认本机验证，统一走 `just msfs <check|build|build-release|package|run|example|example-ai>`；发布前完整核对用 `just check-release`。
- **约定**：版本号只由 `just set-version X.Y.Z` 修改；`just _check-version` 会校验 `Cargo.toml`、`core/src/lib.rs` 与 `web/package.json` 三处一致。
- `core/` —— 数据内核 crate `fly_ruler_proto_core`，含 wire schema、传输、存储、回放与管理接口
- `server/` —— UDP 接收与 HTTP/WS 管理服务，二进制 `fly-ruler-server`
- `bindings/` —— `python`、`godot`、`msfs` 三套绑定
- `web/` —— Vue 3 控制台前端
- `examples/` —— 由简到繁的协议示例，阅读顺序见 `bindings/python/examples/README.md`
- `docs/` —— 用户手册（`docs/guide/`）与开发者手册（`docs/dev/`）

## 文档

- **约定**：正本在 `docs/guide/`、`docs/dev/`；根站 `docs/{guide,dev}/components/proto/` 下的同名页面只是 `<!--@include-->` 壳页，改内容改本目录，新增章节时同时建壳页并在 `docs/.vitepress/config.mts` 登记侧边栏。
- **约定**：面向使用者写安装、连接、状态与事件、遥测、回放、服务与控制台、MSFS 桥接与排障，面向开发者写架构、wire schema、UDP 会话、存储与回放、服务与管理接口、各绑定实现、扩展点与验证；中文散文，段落单行不手工折行，跨页链接用站点绝对路径，行为结论带源码位置（`core/src/...:行号`）。
- **禁止**：正文写主站文档站的结构与转发关系，也不写写作规则、完成状态、进度与占位内容；本仓自己的命令与验证入口是读者需要的内容，可以写。
- **约定**：接口参考页由生成器从源码 docstring 生成，改注释后在主仓重生成（`cd docs && just api`）；漏生成时 `cd docs && just test` 会报错。

## 本目录特有边界

- **必须**：`core/proto/fly_ruler.proto` 是 wire schema 唯一事实源，协议字段、`PROTOCOL_VERSION` 与生成代码同步；生成的 prost/binding 代码不可手改。
- **必须**：UDP session、ACK、heartbeat、best-effort 语义变化有协议回归测试；新增可选字段不得复用已有字段编号。
- **必须**：PyO3 client/server 显式 close 并保持 context-manager 清理语义，关闭后调用抛 `ConnectionError`。
- **必须**：Rust、Python、Godot、MSFS 的字段绑定与文档同步更新；Python 公开面变化同时更新 `_core.pyi`、`bindings/python/examples/` 与 `README.md`。
- **禁止**：core 实现 UI replay、渲染插值或模型绑定；这些职责属于 consumer。
- **禁止**：在内部 workspace crate 新增重复 AGENTS；本文件已覆盖本仓。
- **禁止**：手改 maturin 生成物、锁定依赖或 `web/dist` 产物。
