# 服务端与控制台

`fly-ruler-server` 是 FlyRuler 协议的数据汇聚进程：它用 UDP 接收各客户端上报的飞机状态与遥测，用 HTTP 与 WebSocket 对外提供查询、订阅与回放接口，并可选地托管配套的 Vue 控制台。下面依次说明启动方式、`fly-ruler-server.example.toml` 里每个配置项的含义，以及控制台能做什么、怎么在本地开发。

## 启动服务端

最常用的入口是 `just dev server`，它把参数透传给 `cargo run -p fly_ruler_proto_server`（`justfile:28-29`），编译出的二进制名是 `fly-ruler-server`（`server/Cargo.toml` 的 `[[bin]]` 段）。

```bash
just dev server
just dev server --no-http
just dev server --config server/fly-ruler-server.example.toml
./target/release/fly-ruler-server --log-level debug
```

进程先加载配置、初始化日志，再绑定 UDP 监听地址（`server/src/main.rs:10-20`）；只有管理服务开启时才继续绑定 HTTP/WebSocket 地址（`server/src/main.rs:27-36`）。两条关键启动日志是 `FlyRuler UDP server started` 与 `FlyRuler HTTP/WebSocket management server started`（`server/src/main.rs:25`、`server/src/main.rs:34`）；收到 Ctrl-C 后先停管理服务再停 UDP（`server/src/main.rs:38-42`）。

要让服务端与控制台开发服务器一起启动，直接运行 `just dev`：它在后台跑 `cargo run -p fly_ruler_proto_server`，再进入 `web/` 执行 `pnpm dev`，退出时自动杀掉后端子进程（`justfile:34-40`）。

## 配置文件

配置文件默认取当前目录下的 `fly-ruler-server.toml`，不存在就用内置默认值（`server/src/config.rs:11`、`server/src/config.rs:122-128`）。优先级是命令行参数高于 TOML，TOML 高于内置默认；`--http` 与 `--no-http` 互斥（`server/src/config.rs:37-40`），未知键会直接报错（`server/src/config.rs:62`）。相对路径按启动进程时的工作目录解析（`server/src/config.rs:138`、`server/src/config.rs:296-302`）。

`server/fly-ruler-server.example.toml` 列出下列配置项（其中 `management.public_api_base_url`、`management.public_websocket_url` 与 `logging.file_path` 在示例中被注释掉，需要时再取消注释）：

| 配置项 | 默认值 | 说明 |
| --- | --- | --- |
| `schema_version` | `2` | 必须等于 2，否则报 `unsupported schema_version {n}; expected {m}`（`server/src/config.rs:130-136`） |
| `transport.udp_listen` | `"127.0.0.1:18002"` | UDP 状态接收地址（`server/src/config.rs:146`） |
| `transport.heartbeat_interval_secs` | `5` | 客户端心跳间隔（`core/src/config.rs:66`） |
| `transport.heartbeat_timeout_secs` | `15` | 服务端会话超时（`core/src/config.rs:67`） |
| `cursor_stream.publish_hz` | `30.0` | 游标快照发布频率（`core/src/config.rs:89`） |
| `cursor_stream.max_subscribers` | `16` | 游标房间订阅者上限（`core/src/config.rs:90`） |
| `cursor_stream.reconnect_initial_secs` | `0.5` | 客户端重连初始退避（`core/src/config.rs:91`） |
| `cursor_stream.reconnect_max_secs` | `5.0` | 客户端重连最大退避（`core/src/config.rs:92`） |
| `management.enabled` | `true` | 是否启动 HTTP/WebSocket 管理服务（`server/src/config.rs:152`） |
| `management.listen` | `"127.0.0.1:18003"` | 管理服务监听地址（`server/src/config.rs:157`） |
| `management.data_root` | `"sessions"` | 会话数据根目录（`server/src/config.rs:161`） |
| `management.web_root` | `"web/dist"` | 控制台静态站点根目录（`server/src/config.rs:167`） |
| `management.public_api_base_url` | 无 | 注入控制台的公开 API 前缀，缺省 `/api/v1`（`core/src/management/server.rs:419`） |
| `management.public_websocket_url` | 无 | 注入控制台的公开 WebSocket 地址，缺省 `/api/v1/ws`（`core/src/management/server.rs:420`） |
| `management.websocket_hz` | `30.0` | WebSocket 推送频率（`server/src/config.rs:173`） |
| `management.cors_origins` | 8 条本地来源 | 允许的浏览器来源（`core/src/config.rs:217-226`） |
| `playback.default_speed` | `1.0` | 回放初始倍速（`core/src/config.rs:245`） |
| `playback.min_speed` | `0.1` | 最小倍速（`core/src/config.rs:246`） |
| `playback.max_speed` | `16.0` | 最大倍速（`core/src/config.rs:247`） |
| `logging.level` | `"warn"` | 日志级别（`server/src/config.rs:211`） |
| `logging.file_path` | 无 | 写入文件而不是 stderr，父目录会自动创建（`core/src/logging.rs:31-50`） |

示例文件把 `logging.level` 设为 `info`，并显式列出 `cors_origins` 只有 5173 两条来源；`cursor_stream.reconnect_initial_secs` 与 `cursor_stream.reconnect_max_secs` 会被解析但服务端不消费它们，运行配置里的对应字段用内置默认补齐（`server/src/config.rs:267-271`）。

## 配置校验

加载阶段会拒绝一组非法组合，报错文本可直接定位到键名（`server/src/config.rs:218-256`）：`transport.udp_listen` 不能为空；管理服务开启时 `management.listen` 不能为空；`heartbeat_interval_secs` 必须大于零且 `heartbeat_timeout_secs` 必须严格大于它；`cursor_stream.publish_hz` 必须有限且大于零；`max_subscribers` 必须大于零；`management.websocket_hz` 必须有限且大于零；倍速必须满足 `0 < min_speed <= default_speed <= max_speed`；`logging.level` 只能是 `trace`、`debug`、`info`、`warn`、`error`。

日志默认级别里的 `warn` 来自服务端内置默认（`server/src/config.rs:211`），而内核库的默认过滤串会在 `warn` 基线上把 `fly_ruler_proto_core.runtime`、`fly_ruler_proto_core.store` 提到 `info`、把 `fly_ruler_proto_core.transport` 压到 `warn`（`core/src/logging.rs:15-18`）。设置 `RUST_LOG` 会覆盖这个默认串（`core/src/logging.rs:69-70`）。

## HTTP 与 WebSocket 管理接口

所有接口挂在 `/api/v1` 前缀下（`core/src/management/routes.rs:33-57`）。这里只列端点与用途，请求与响应细节见开发者手册。

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/api/v1/health` | 健康检查，返回 `status`、`protocol_version`、`api_version`（`core/src/management/routes.rs:75-79`） |
| GET | `/api/v1/status` | 运行状态，含回放模式与数据边界 |
| GET | `/api/v1/aircraft` | 飞机列表 |
| GET | `/api/v1/aircraft/{id}/state` | 某架飞机的当前状态 |
| GET | `/api/v1/aircraft/{id}/states` | 某架飞机的状态分页 |
| GET | `/api/v1/aircraft/{id}/events` | 某架飞机的事件 |
| GET | `/api/v1/timeline/events` | 跨飞机的事件时间线 |
| GET | `/api/v1/aircraft/{id}/series/catalog` | 该机可查询的时间序列目录 |
| POST | `/api/v1/series/query` | 按选择项与时间范围取序列点 |
| GET | `/api/v1/playback` | 当前回放状态 |
| POST | `/api/v1/playback/live` | 切回实时 |
| POST | `/api/v1/playback/pause` | 暂停回放 |
| POST | `/api/v1/playback/play` | 开始或继续回放 |
| POST | `/api/v1/playback/seek` | 定位到指定时间 |
| POST | `/api/v1/playback/step` | 按样本或事件单步 |
| PUT | `/api/v1/playback/speed` | 设置回放倍速 |
| POST | `/api/v1/memory/clear` | 清空内存中的时间序列 |
| GET | `/api/v1/sessions` | 会话列表 |
| POST | `/api/v1/sessions/{name}/save` | 保存会话 |
| POST | `/api/v1/sessions/{name}/load` | 载入会话 |
| GET | `/api/v1/operations/{id}` | 查询异步操作状态 |
| GET、PUT | `/api/v1/workspace` | 读取或保存控制台工作区 |
| GET | `/api/v1/ws` | WebSocket 快照与控制游标推送 |

未登记的 `/api` 路径统一返回 404，方法不匹配返回 405（`core/src/management/routes.rs:58-72`）。回放错误会映射成 HTTP 状态：数据为空时是 409，时间戳、倍速或步数非法时是 400（`core/src/management/server.rs:595-602`）。

## 控制台能做什么

控制台是随服务端分发的 Vue 3 单页应用，源码在 `web/src`。连上管理服务后，它通过 WebSocket 接收 `snapshot`、`operation_status`、`store_changed`、`workspace_changed` 四类推送（`web/src/stores/server.ts`），启动时并行拉取状态、飞机列表、会话列表与时间线（`web/src/App.vue`）。

- 连接服务端：地址来自页面注入的运行配置，默认 API 前缀 `/api/v1`、WebSocket 地址 `/api/v1/ws`（`web/src/runtime.ts`）。
- 飞机列表与数据篮：左侧 `DataSidebar` 列出飞机与可画字段，右侧 `InspectorPanel` 显示单点详情。
- 时间序列图表：`ChartWorkspace` 与 `ChartCard` 用 ECharts 画曲线，实时模式下随快照递增补点，存储版本变化时清空重画（`web/src/App.vue`）。
- 回放控制：`PlaybackToolbar` 与 `TimelineBar` 提供实时、暂停、播放、定位、单步与倍速。
- 快捷键：空格切换播放，方向键左右单步样本（按住 Shift 为 10 步）、上下单步事件、Home 与 End 跳到边界（`web/src/shortcuts.ts`）。

## 本地开发与构建

前端开发服务器是 Vite，默认端口 5173，并把 `/api` 代理到 `http://127.0.0.1:18003` 且开启 WebSocket 转发（`web/vite.config.ts`）。

```bash
just dev                     # 同时起后端与 Vite
just dev web                 # 只起 Vite，代理到已有的管理服务
just dev server              # 只起后端
cd web && pnpm build         # vue-tsc -b && vite build
just build                   # cargo build --workspace 加 web/dist
```

构建产物是 `web/dist`，当 `management.web_root` 下存在 `index.html` 时由管理服务直接托管；没有这个文件就只提供 API，不返回页面（`core/src/management/server.rs:404-410`）。页面里的运行配置标记若缺失，服务端会报 `does not contain the runtime configuration placeholder`（`core/src/management/server.rs:412-417`）。

## 相关页面

- [遥测与时间序列](/guide/components/proto/03-telemetry)
- [回放](/guide/components/proto/04-playback)
- [服务与管理接口实现](/dev/components/proto/05-server-http-ws)
- [控制台前端实现](/dev/components/proto/07-console-frontend)
- [接口参考](/api/proto/)
