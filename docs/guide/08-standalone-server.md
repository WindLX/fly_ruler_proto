# 无 MSFS 时用独立服务端

## 什么时候用

手边没有模拟器、要做集成测试、或者只想打开控制台看数据时，用独立服务端 `fly-ruler-server`。它提供与 MSFS 桥完全相同的 UDP 接入和管理接口，只是没有 SimConnect 那一侧。

用独立服务端可以先用脚本灌一段假数据把控制台、图表、会话和回放串起来，等真正接入模拟器时只换数据来源。

做集成测试时它比启动模拟器便宜得多，而且行为可重复：同样的脚本、同样的时间戳，跑出来的会话每次都能对比。

它也可以只当 UDP 接收端用，用 `--no-http` 关掉管理面，配合自己的消费者处理数据。

## 启动

从 Release 包解压后直接运行 `fly-ruler-server`，默认读取运行目录下的 `fly-ruler-server.toml`。

在源码树里用 `just dev server`，它执行 `cargo run -p fly_ruler_proto_server -- {{ARGS}}`（`justfile:31-32`），后面的参数原样传给服务端。

服务端启动时先看 `--config` 指定的文件，没有指定就找运行目录下的 `fly-ruler-server.toml`（常量名见 `server/src/config.rs:11`），存在就用它，不存在则退回内置默认值（`server/src/config.rs:117-129`）。

客户端连接地址要与服务端 UDP 监听一致，换端口时两端同时改。

命令行常用的覆盖项有 `--udp-listen`、`--http-listen`、`--data-root`、`--web-root`（`server/src/config.rs:18-28`），以及 `--http` 与 `--no-http` 这一对互斥开关（`server/src/config.rs:37-40`）。

启动成功后日志打印 `FlyRuler UDP server started`（`server/src/main.rs:22-26`），管理面打开时再打印 `FlyRuler HTTP/WebSocket management server started`（`server/src/main.rs:27-36`）；按 `Ctrl-C` 会依次停掉管理面与 UDP 接收（`server/src/main.rs:38-42`）。

## 配置

配置模板是 `server/fly-ruler-server.example.toml`，按段组织传输、游标流、管理面、回放与日志。

模板开头的 `schema_version = 2` 标记配置结构版本（`server/fly-ruler-server.example.toml:1`），改配置格式时据此判断兼容性。

`[transport]` 段管 UDP 监听与心跳周期，`[cursor_stream]` 段管游标推送频率与订阅上限，`[management]` 段示例里把 `cors_origins` 配成允许本机 5173 的开发前端（`server/fly-ruler-server.example.toml:3-26`）。

默认 UDP 监听 `127.0.0.1:18002`（`server/src/config.rs:143-146`），管理面监听 `127.0.0.1:18003`（`server/src/config.rs:154-157`）。

管理面是否启用按固定顺序判定：命令行的 `--http` 强制打开，`--no-http` 强制关闭，两者都没有时读取配置里的 `management.enabled`，缺省为打开（`server/src/config.rs:147-153`）。

数据目录默认 `sessions`（`server/src/config.rs:158-163`），前端目录默认 `web/dist`（`server/src/config.rs:164-169`）；模板注释说明这两个相对路径都相对服务端启动目录解析（`server/fly-ruler-server.example.toml:17`）。

回放的倍速范围取自配置，示例给的是 `default_speed = 1.0`、`min_speed = 0.1`、`max_speed = 16.0`（`server/fly-ruler-server.example.toml:28-31`），加载时校验 `0 < min <= default <= max`（`server/src/config.rs:239-250`）。

日志级别默认 `warn`（`server/src/config.rs:207-211`），可以用 `RUST_LOG` 覆盖（`core/src/logging.rs:66-70`）。

## 它和 MSFS 桥是同一个服务端

两种部署共用同一个内核运行时。独立服务端在 `server/src/main.rs:19-36` 里先 `KernelRuntime::with_config`，再依次启动 UDP 与管理面；MSFS 桥在 `bindings/msfs/src/bridge.rs:15-45` 做同样的两步，只是把 SimConnect 循环放在旁边。

两边也共用同一个 `TimeSeriesStore` 实现（`server/src/main.rs:18`），状态、事件与遥测的存储格式和查询接口都一致。

管理面的启动入口都是 `KernelRuntime::start_management_server`（`core/src/kernel.rs:149-167`），注册的路由表也是同一份（`core/src/management/routes.rs:33-57`）。

差别只在配置来源与调用点：独立服务端读 `fly-ruler-server.toml` 与命令行，桥读 `fly-ruler-msfs.toml` 与桥自己的命令行参数。

因此控制台、字段目录、会话保存与回放、遥测的用法在两种部署下完全一致，切换部署不需要改客户端代码，只改连接地址。

协议报文与管理接口的细节属于开发内容，写在 `docs/dev/advanced/udp-session.md` 与 `docs/dev/advanced/management-api.md` 里。

## 跑示例

`bindings/python/examples/01_connect_and_push.py` 是端到端示例：它完成握手、打印协议版本与两个会话 UUID，然后按 `--hz` 持续推送状态，服务端立刻开始记时间序列。

先启动服务端，再在 `bindings/python` 目录下运行 `uv run python examples/01_connect_and_push.py`，换地址时用 `--address`。

想看事件与遥测换 `03_events_and_telemetry.py`，想看长时回放换 `04_sessions_and_playback.py`，多机场景用 `05_multi_aircraft.py`，MSFS 相关的 `02_control_msfs.py` 与 `06_ai_fleet_msfs.py` 需要桥接在跑。

## 相关页面

- [安装](01-install.md)
- [控制台](03-console.md)
- [会话、回放与导出](06-sessions.md)
