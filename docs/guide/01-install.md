# 安装与快速开始

FlyRuler proto 提供 Rust 数据内核、独立的 UDP 与管理服务进程，以及 Python、Godot、MSFS 三套绑定；组件全貌见 [proto 组件概览](/guide/components/proto)。

最常用的入门路径是：安装 Python 绑定、启动 `fly-ruler-server`、用 `FlyRulerClient` 把一架飞机的状态推给服务端。

## 安装 Rust 内核 crate

```toml
[dependencies]
fly_ruler_proto_core = "0.4"
```

用 `cargo add fly_ruler_proto_core` 即可添加依赖，包名与该 crate 的库名一致（`core/Cargo.toml:2`）。

协议的语义版本以常量的形式导出，值为 `"0.4.0"`（`core/src/lib.rs:34`）。

```rust
use fly_ruler_proto_core::PROTOCOL_VERSION;

fn main() {
    println!("protocol {PROTOCOL_VERSION}");
}
```

工作区版本号定义在 `Cargo.toml:6`，并由 `just set-version` 同步到内核与 Web 包。

> 注册表上可安装到的版本可能低于当前源码版本。握手会校验两端的协议版本必须一致（`core/src/transport/server.rs:445-467`），所以客户端与服务端要取自同一版本。

## 安装 Python 绑定

```bash
python -m pip install fly_ruler_proto_python
```

分发名为 `fly_ruler_proto_python`（`bindings/python/pyproject.toml:2`），运行时要求 Python 3.12 或更新（`bindings/python/pyproject.toml:9`）。

安装后可直接运行同名命令，它会打印协议版本与全部公开导出项：

```bash
fly_ruler_proto_python
```

该入口定义在 `bindings/python/pyproject.toml:12-13`，实现见 `bindings/python/src/fly_ruler_proto_python/__init__.py:61-71`。公开面包含 14 个名字，例如 `FlyRulerClient`、`create_aircraft_state`、`Attitude`、`TelemetryStreamSchema`。

## 从源码安装

克隆仓库后在 `packages/fly_ruler_proto` 下执行：

```bash
just setup
```

`setup` 由 `_setup-python` 与 `_setup-web` 组成（`justfile:9`）：前者在 `bindings/python` 里跑 `uv sync --all-groups`（`justfile:79-80`），后者在 `web` 里跑 `pnpm install`（`justfile:82-83`）。本机需要 Rust 工具链、uv 与 pnpm。

| 命令 | 作用 |
| --- | --- |
| `just check` | 版本一致性加上 Rust、Python、Web 三面的检查（`justfile:15`） |
| `just test` | 三套测试；Python 那套会先跑 `maturin develop`（`justfile:18`、`:127`） |
| `just build` | `cargo build --workspace` 与控制台静态资源（`justfile:21`） |
| `just fmt` | 三面的格式化落盘（`justfile:12`） |
| `just pre-commit` | 依次执行 `fmt`、`check`、`test`（`justfile:73`） |

开发控制台时用 `just dev`：不带参数会同时启动服务端与 Vite 开发服务器，`just dev server` 只起服务端，`just dev web` 只起前端（`justfile:24-45`）。

## 启动服务端

```bash
just dev server
```

它等价于 `cargo run -p fly_ruler_proto_server`（`justfile:28-29`），也可以直接运行编译好的 `fly-ruler-server` 二进制。

默认监听 UDP `127.0.0.1:18002` 与管理接口 `127.0.0.1:18003`（`server/src/config.rs:143-157`）。配置可以来自命令行参数，也可以来自配置文件：当前目录下的 `fly-ruler-server.toml` 会被自动读取（`server/src/config.rs:11`，`server/src/config.rs:122-129`），完整模板见 `server/fly-ruler-server.example.toml`。

```toml
schema_version = 2

[transport]
udp_listen = "127.0.0.1:18002"

[management]
enabled = true
listen = "127.0.0.1:18003"
```

`schema_version` 必须等于 2（`core/src/config.rs:9`）。日志级别设为 `info` 时，启动过程会打印 `UDP server runtime started`（`core/src/kernel.rs:129`）与 `management server started`（`core/src/management/server.rs:276`）。

服务起来后先用健康检查确认：

```bash
curl -s http://127.0.0.1:18003/api/v1/health
# {"status":"ok","protocol_version":"0.4.0","api_version":"v1"}
```

## 最小 Python 客户端

下面的脚本连接服务端、生成一架飞机、更新一次状态，并在退出时关闭会话：

```python
from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state

with FlyRulerClient("127.0.0.1:18002", "quickstart") as aircraft:
    state = create_aircraft_state(
        position=(100.0, 0.0, -1000.0),
        velocity=(200.0, 0.0, 0.0),
        angular_velocity=(0.0, 0.0, 0.05),
    )
    aircraft.update_state(state)
    print("aircraft_uuid:", aircraft.aircraft_uuid)
```

构造 `FlyRulerClient` 时会建立 UDP 会话并完成握手，握手等待 ACK 的上限是 1 秒（`core/src/transport/client.rs:18`，`core/src/transport/client.rs:276-287`）；`with` 块退出时调用 `close()`，若之前没有显式 despawn，会补发一条 reason 为 `client_close` 的 despawn（`core/src/transport/client.rs:626-634`）。

`update_state(state)` 不传时间戳时由客户端填当前 Unix 秒（`core/src/transport/client.rs:36-46`）。

这段代码要求服务端已经在 `127.0.0.1:18002` 上监听 UDP：服务端没启动时构造会直接抛出连接异常，构造失败不会留下半初始化实例（`bindings/python/src/fly_ruler_proto_python/client.py:86-89`）。

确认数据已到达服务端：

```bash
curl -s http://127.0.0.1:18003/api/v1/aircraft
```

状态更新的时间戳语义、心跳与关闭顺序见 [客户端连接与状态更新](/guide/components/proto/02-client-connection)。

配套的可运行示例是 `bindings/python/examples/01_connect.py`；示例清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [客户端连接与状态更新](/guide/components/proto/02-client-connection)
- [遥测与时间序列](/guide/components/proto/03-telemetry)
- [UDP 会话与错误处理](/dev/components/proto/03-udp-session)
- [proto 组件概览](/guide/components/proto)
- [协议 API 参考](/api/proto/)
