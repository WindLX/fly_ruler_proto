# fly_ruler_proto

Protobuf/UDP protocol and data kernel for FlyRuler flight simulations.

`fly_ruler_proto` 把飞行仿真器与外部消费者接到同一条协议上：仿真器按固定频率上报飞机状态与自定义事件，遥测按声明式 schema 独立成流，服务端把状态组织成时间序列供控制台查询与回放。wire schema 是唯一事实源，各语言绑定只做类型映射与生命周期封装。

## 能做什么

- 用 protobuf 描述飞机状态、事件、遥测与协议消息，全部走 UDP 会话，含 ACK、心跳与 best-effort 语义。
- 把每架飞机的状态写入按时间分片的时间序列，提供目录、查询、分页与游标订阅。
- 用 HTTP/WebSocket 管理接口提供图表数据、回放控制、工作区读写与运行配置注入。
- 附带一个 Vue 3 控制台，可直接浏览实时曲线、选择字段、切换回放速度。
- 提供 Python、Godot 与 MSFS 绑定：Python 面向脚本与自动化，MSFS 把状态桥接进模拟器并支持 AI 机队。

本仓库不包含飞行动力学模型：模型通过协议把状态发进来，或者由消费者把状态映射到模拟器。

## 组成

- `core/` —— Rust crate `fly_ruler_proto_core`，含 wire schema、UDP 传输、时间序列存储、回放与管理 API。
- `server/` —— 服务进程 `fly-ruler-server`，同时提供 UDP 接入与 HTTP/WebSocket 管理接口。
- `bindings/python/` —— PyO3 绑定与 Python 高层客户端。
- `bindings/godot/` —— Godot 4 GDExtension，当前仅 Linux x86_64，接口仍在演进。
- `bindings/msfs/` —— MSFS 2024 SimConnect 桥接，面向 Windows 与 Proton。
- `web/` —— Vue 3 控制台前端，构建产物由服务进程托管。

## 安装

内核 crate 从 crates.io 获取：

```bash
cargo add fly_ruler_proto_core
```

Python 绑定从 PyPI 获取，发行名是 `fly_ruler_proto_python`，导入名相同：

```bash
pip install fly_ruler_proto_python
```

需要 Python 3.12+。Windows 与 Linux 都有 wheel，Linux wheel 为 manylinux 产物。

服务进程从 GitHub Release 下载预编译包后解压运行：

```bash
tar -xzf fly-ruler-server-linux-x86_64.tar.gz
./fly-ruler-server --config fly-ruler-server.toml
```

在源码仓库里开发时，用本目录的 justfile 同步依赖并本地安装扩展模块：

```bash
cd packages/fly_ruler_proto
just setup
```

## 快速开始

先启动服务，默认监听 UDP `127.0.0.1:18002` 与管理接口 `127.0.0.1:18003`，配置文件缺省的路径是运行目录下的 `fly-ruler-server.toml`：

```bash
just dev server
```

再用客户端接入，构造客户端即完成握手并注册飞机：

```python
from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state

with FlyRulerClient("127.0.0.1:18002", "F-16") as aircraft:
    aircraft.update_state(create_aircraft_state(position=(100.0, 0.0, -1000.0)))
    aircraft.create_event("flyruler.control.gear_down")
```

浏览器打开 `http://127.0.0.1:18003` 就是控制台，可以看到刚注册的飞机与它的状态曲线。`close()` 幂等，`with` 退出后继续调用方法会得到 `ConnectionError`。

## 示例

`bindings/python/examples/` 按由简到繁排列，从连接与推送状态，到事件、遥测、多机并发、圆周飞行，再到 MSFS 桥接与 AI 机队；每个脚本对应 `docs/guide/` 的一章。阅读顺序与运行前提见 `bindings/python/examples/README.md`。

## 文档与开发

- 用户手册：`docs/guide/` —— 安装、连接、遥测、回放、服务与控制台、MSFS 桥接与排障。
- 开发者手册：`docs/dev/` —— 架构与分层、wire schema、UDP 会话、存储与回放、服务与管理接口、各语言绑定实现、扩展点与验证。
- 接口参考：`docs/dev/api.md` —— Python 公开面与 Rust 公开模块的索引，签名细节见自动生成的 API 参考。
- 发布流程：`RELEASING.md` —— 版本修改、门禁、打标签与各分发渠道的发布步骤。
- 命令以 `just --list` 为准：`just setup`、`just fmt`、`just check`、`just test`、`just build`、`just pre-commit`；开发期起服务与控制台用 `just dev`，MSFS 与 Windows 步骤用 `just msfs <任务>`。
