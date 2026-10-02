# 排障

FlyRuler 的故障大多落在三类边界上：UDP 会话没建立、状态或遥测被内核丢弃、以及浏览器或模拟器一侧没接上。下面按症状组织，每条给出症状、常见原因、可粘贴的检查命令与处理办法。

## 客户端连不上服务端

**症状**：`FlyRulerClient(...)` 构造直接抛出 `ConnectionError`，消息是 `handshake timed out waiting for server ACK`。

**原因**：UDP 没有连接建立这一步，地址写错、服务端没起、端口被别的进程占用，都会表现成握手超时。客户端发出握手后只等 1 秒就放弃（`core/src/transport/client.rs:18`），超时错误文本定义在 `core/src/transport.rs:40-41`，最终映射成 `ConnectionError`（`bindings/python/src/client.rs:142`）。

**检查**：

```bash
ss -lunp | grep 18002
ss -ltnp | grep 18003
curl -s http://127.0.0.1:18003/api/v1/health
```

**处理**：确认服务端已打印 `FlyRuler UDP server started`，并让客户端地址与服务端 `transport.udp_listen` 完全一致；端口冲突时换端口并同步改两端。

## 协议版本不匹配

**症状**：同样是 `ConnectionError`，但消息含 `code=... message=protocol version mismatch`。

**原因**：服务端在收到握手时比较 `hs.version` 与自身 `PROTOCOL_VERSION`（`core/src/transport/server.rs:446`），不一致就回 `ProtocolVersionMismatch` 并带文本 `protocol version mismatch`（`core/src/transport/server.rs:460-466`）；客户端要求应答必须是成功 ACK，否则抛出 `HandshakeRejected`，格式是 `code={} message={}`（`core/src/transport/client.rs:256-274`）。

**检查**：

```bash
cd bindings/python && uv run fly_ruler_proto_python
curl -s http://127.0.0.1:18003/api/v1/health
```

**处理**：两侧的 `protocol_version` 必须相同；把客户端与服务端都从同一版本重新构建，不要混用旧 wheel 与新二进制。

## 心跳超时或丢包

**症状**：长时间运行后客户端仍在发状态，但服务端日志出现 `session expired and removed`。

**原因**：只有握手、心跳、游标订阅这类请求会收到 ACK，普通状态更新是 best-effort，不回 ACK（`core/src/transport/server.rs:468-511`）。服务端会清理超过 `heartbeat_timeout_secs` 没消息的会话（`core/src/transport/server.rs:258-286`、`core/src/transport/server.rs:308`）。

**检查**：

```bash
cargo run -p fly_ruler_proto_server -- --log-level info
# 确认 heartbeat_timeout_secs 严格大于 heartbeat_interval_secs
grep -n "heartbeat" server/fly-ruler-server.example.toml
```

**处理**：保持 `heartbeat_timeout_secs > heartbeat_interval_secs`，跨 NAT 或无线链路时把超时调大；客户端心跳周期由 `heartbeat_interval_secs` 控制，默认 1 秒。

## 遥测丢数据

**症状**：曲线里缺少某个遥测流，或该流一条点都没有。

**原因**：遥测流必须在飞机生成时用 schema 登记，登记失败时整次生成被忽略并记录 `ignored spawn with invalid telemetry schema`（`core/src/store.rs:365-366`）；帧字段非法时被丢弃并记录 `ignored invalid telemetry frame`（`core/src/store.rs:385`）；上报了未登记的流则报 `unknown telemetry stream: {stream_id}`（`core/src/store.rs:1120`）。

**检查**：

```bash
cargo run -p fly_ruler_proto_server -- --log-level info
# 观察 ignored invalid telemetry frame / unknown telemetry stream
curl -s http://127.0.0.1:18003/api/v1/aircraft
```

**处理**：把发送端声明的 schema 与上报帧的 `stream_id`、字段类型逐一对齐；schema 只在生成时登记一次，生成之后再补不会生效。

## 回放游标不前进

**症状**：按下播放后时间轴停在原地，或立即弹回实时。

**原因**：存储里没有任何样本时播放会被拒绝，返回 409（`core/src/management/server.rs:595-602`、`core/src/playback.rs:168-188`）；实时模式会把游标设到数据区间末端（`core/src/playback.rs:132-138`），没有区间时游标为空。

**检查**：

```bash
curl -s http://127.0.0.1:18003/api/v1/status
curl -s http://127.0.0.1:18003/api/v1/playback
```

**处理**：先让生产者上报一段数据，再切到回放；用定位或单步确认游标能落在区间内。控制台的倍速受 `playback.min_speed` 与 `playback.max_speed` 限制。

## 控制台空白或连不上

**症状**：打开管理地址是空白页或 404，控制台里所有面板都报错。

**原因**：管理服务只在 `management.web_root` 下存在 `index.html` 时才托管前端，否则只提供 API（`core/src/management/server.rs:404-410`）；页面缺少运行配置标记时会报 `does not contain the runtime configuration placeholder`（`core/src/management/server.rs:412-417`）。开发模式下 Vite 把 `/api` 代理到 `http://127.0.0.1:18003`（`web/vite.config.ts`）。

**检查**：

```bash
ls web/dist/index.html
curl -s http://127.0.0.1:18003/ | head -n 5
cd web && pnpm dev
```

**处理**：先 `just build` 生成 `web/dist`，再启动服务端；用 Vite 开发服务器时确认后端在 18003 端口监听，且浏览器访问的是 5173。

## Python 自检入口失败

**症状**：`uv run fly_ruler_proto_python` 报导入错误，或整个包 `import` 失败。

**原因**：这个入口只打印协议版本与公开导出面，不连服务端（`bindings/python/src/fly_ruler_proto_python/__init__.py:61-71`）；它依赖 Rust 扩展 `fly_ruler_proto_python._core`，扩展没编译或 ABI 不匹配时导入即失败。

**检查**：

```bash
cd bindings/python && uv run maturin develop
cd bindings/python && uv run fly_ruler_proto_python
```

**处理**：先用 `uv run maturin develop` 把扩展编译进当前虚拟环境（`just test` 的 Python 那套也会顺带编译），再重跑自检；正常输出首行是 `fly_ruler_proto_python 协议版本：0.4.0`，随后是函数返回的版本号与逐项列出的公开名字。

## MSFS 连不上

**症状**：桥接日志每秒重复 `waiting for MSFS 2024 SimConnect`，或者进程直接以状态码 2 退出。

**原因**：桥接在循环里反复尝试连接，失败就等 1 秒重试（`bindings/msfs/src/bridge.rs:64-70`）；非 Windows 目标编译出的产物不会连模拟器，而是打印提示并退出（`bindings/msfs/src/main.rs:15-26`）。

**检查**：

```bash
protontricks-launch --appid 2537590 \
  target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe \
  --log-level debug
ls "SimConnect SDK/include/SimConnect.h"
```

**处理**：确认用的是 Windows 目标产物并经 Proton 启动，MSFS 已进入 Free Flight 且关闭 Active Pause；构建报找不到头文件时设置 `MSFS2024_SDK`。连上后若日志出现 `aircraft state is stale; holding final MSFS pose`，说明生产端停更了，先恢复状态上报。

## 相关页面

- [服务端与控制台](/guide/components/proto/05-server-and-console)
- [MSFS 2024 桥接](/guide/components/proto/06-msfs-bridge)
- [服务与管理接口实现](/dev/components/proto/05-server-http-ws)
- [Python 绑定实现](/dev/components/proto/06-python-binding)
- [接口参考](/api/proto/)
