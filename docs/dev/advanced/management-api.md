# 管理服务与 HTTP/WS 路由

这一页面向协议实现者与二次开发者，逐条列出管理路由、请求体、错误码与 WebSocket 推送帧；只用 Python client 的用户请读使用手册 `docs/guide/01-install.md`。

`fly-ruler-server` 进程在同一个数据内核上开两个监听：UDP 端口接收飞行模型上报，管理端口提供 REST 与 WebSocket。管理面本身是 core 里的一个模块，server 只负责把配置翻译成内核配置并决定是否启用它（`server/src/main.rs:18-36`）。

## 组装

`KernelRuntime::with_config` 创建共享的 store、回放控制器、摄取闸门与会话表的槽位（`core/src/kernel.rs:60-77`）。`start_server` 在启动 UDP 接收循环后把 store 与摄取的写入闭包交给传输层，同时启动游标流运行时并保存一份 `SessionHandle`（`core/src/kernel.rs:102-131`）。

`start_management_server` 把这五份共享状态交给 `ManagementServerRuntime::start`（`core/src/kernel.rs:152-167`）。后者校验配置、解析并创建数据根目录、绑定 TCP 端口，构造 `AppState` 与路由，再用 `axum::serve` 配合取消令牌做优雅停机（`core/src/management/server.rs:240-283`）。停机时先取消服务任务，再等待进行中的持久化操作结束（`core/src/management/server.rs:291-297`）。

`AppState` 的内容就是管理面能触达的全部资源：

| 字段 | 内容 |
| --- | --- |
| `store` | 共享时间序列 store（`core/src/management/server.rs:95`） |
| `playback` | 回放控制器（`core/src/management/server.rs:96`） |
| `ingestion` | 摄取闸门与丢弃计数（`core/src/management/server.rs:97`） |
| `sessions` | 传输层会话表句柄（`core/src/management/server.rs:98`） |
| `config` | 管理面配置（`core/src/management/server.rs:99`） |
| `operations` | 持久化操作管理器与通知广播（`core/src/management/server.rs:100`） |
| `workspace` | 工作区文档存储（`core/src/management/server.rs:101`） |
| `shutdown` | 停机取消令牌（`core/src/management/server.rs:102`） |

## 路由族

路由全部在 `routes::router` 中注册（`core/src/management/routes.rs:33-61`）。

| 方法 | 路径 | 作用 |
| --- | --- | --- |
| GET | `/api/v1/health` | 版本与存活（`core/src/management/routes.rs:35`） |
| GET | `/api/v1/status` | 版本、store 统计、回放快照、摄取状态、UDP 会话（`:36`） |
| GET | `/api/v1/aircraft` | 飞机清单与统计（`:37`） |
| GET | `/api/v1/aircraft/{id}/state` | 单机状态，可用 `at` 指定时间（`:38`） |
| GET | `/api/v1/aircraft/{id}/states` | 状态窗口分页（`:39`） |
| GET | `/api/v1/aircraft/{id}/events` | 事件窗口分页（`:40`） |
| GET | `/api/v1/timeline/events` | 全局事件窗口分页（`:41`） |
| GET | `/api/v1/aircraft/{id}/series/catalog` | 该机可选字段目录（`:42`） |
| POST | `/api/v1/series/query` | 按选择器取窗口序列并降采样（`:43`） |
| GET | `/api/v1/playback` | 回放快照（`:44`） |
| POST | `/api/v1/playback/live` | 回到实时（`:45`） |
| POST | `/api/v1/playback/pause` | 暂停在当前游标（`:46`） |
| POST | `/api/v1/playback/play` | 开始播放（`:47`） |
| POST | `/api/v1/playback/seek` | 跳转并暂停（`:48`） |
| POST | `/api/v1/playback/step` | 逐样本或逐事件移动（`:49`） |
| PUT | `/api/v1/playback/speed` | 修改倍速（`:50`） |
| POST | `/api/v1/memory/clear` | 清空内存数据（`:51`） |
| GET | `/api/v1/sessions` | 列出数据根目录下的会话（`:52`） |
| POST | `/api/v1/sessions/{name}/save` | 异步保存为会话（`:53`） |
| POST | `/api/v1/sessions/{name}/load` | 异步加载会话（`:54`） |
| GET | `/api/v1/operations/{id}` | 查询持久化操作状态（`:55`） |
| GET / PUT | `/api/v1/workspace` | 读取或保存工作区（`:56`） |
| GET | `/api/v1/ws` | WebSocket 推送（`:57`） |

`/api` 本身与 `/api/{*path}` 都落到 404，方法不匹配时回 405（`core/src/management/routes.rs:58-60`）。`GET /api/v1/status` 里的 `udp_sessions` 来自传输层会话表，字段是 `addr`、`client_uuid_hex` 与 `last_seen_secs`（`core/src/management/routes.rs:83-103`）。

## 请求与错误

需要请求体的写操作都经 `json_body` 提取，缺失或不合法时回 400 `invalid_json`；查询参数经 `query_body` 提取，失败回 400 `invalid_query`（`core/src/management/server.rs:441-450`）。请求体都要求 `Content-Type: application/json`。

| 端点 | 请求体 |
| --- | --- |
| `playback/play` | `{"speed": 1.5}`，字段可省（`core/src/management/routes.rs:292-295`） |
| `playback/seek` | `{"timestamp": 12.5}`（`:308-311`） |
| `playback/step` | `{"unit": "sample", "direction": "next", "count": 1}`（`:324-329`） |
| `playback/speed` | `{"speed": 2.0}`（`:342-345`） |
| `memory/clear` | `{"confirm": true}`，非真回 400 `confirmation_required`（`:358-372`） |
| `sessions/{name}/save` | `{"overwrite": true}`，字段可省，默认不覆盖（`:417-421`） |
| `series/query` | `selections`、可选 `time_range` 与 `max_points`（`core/src/management/series.rs:134-144`） |
| `workspace` | 完整工作区快照（`core/src/management/routes.rs:552-556`） |

错误统一是 `ApiError`，带状态码、`code`、`message` 与可选 `details`（`core/src/management/server.rs:540-546`）。回放错误映射为 409 或 400，序列错误里飞机不存在映射为 404、其余映射为 400，工作区错误按过大、过复杂与取值非法分别映射（`core/src/management/server.rs:595-628`）。

## WebSocket 推送

`GET /api/v1/ws` 支持 `aircraft` 查询参数，用逗号分隔要跟踪的飞机 ID；不传则跟踪 store 里的全部飞机（`core/src/management/routes.rs:566-589`）。升级后单帧上限 16 KiB，超出会让连接失败（`core/src/management/routes.rs:587`）。

连接建立后先发一条 `hello`，含 API 版本、协议版本与服务器时间（`core/src/management/routes.rs:592-601`）。之后按 `management.websocket_hz` 的节拍发 `snapshot`，包含递增序号、服务器时间、回放快照、store 统计、每架飞机的解析结果与是否被截断的标志（`core/src/management/routes.rs:606-686`）。飞机数量超过 64 时只保留前 64 架并把 `truncated` 置真（`core/src/management/routes.rs:655-658`）。

| 帧类型 | 触发 |
| --- | --- |
| `hello` | 连接建立（`core/src/management/routes.rs:594-600`） |
| `snapshot` | 固定节拍（`core/src/management/routes.rs:614-620`） |
| `operation_status` | 持久化操作入队或状态变化（`core/src/management/server.rs:154-175`） |
| `store_changed` | 清空或加载完成后（`core/src/management/routes.rs:380`、`:517`） |
| `workspace_changed` | 工作区保存后带新修订号（`core/src/management/routes.rs:562`） |
| `error` | 客户端发送文本或二进制帧（`core/src/management/routes.rs:637-646`） |

通道是单向推送：客户端发来的文本或二进制帧会得到 `websocket_read_only` 错误帧，提示回放命令要走 REST（`core/src/management/routes.rs:638-642`）。Ping 会得到 Pong，Close 或服务器停机都会结束循环（`core/src/management/routes.rs:628-652`）。

## 写操作的门控

数据摄取由 `IngestionGate` 控制。每条 UDP 上报在共享许可下写入 store；闸门关闭期间到达的上报只增加丢弃计数，写入动作被跳过（`core/src/management/gate.rs:30-46`）。需要一致性快照的操作在禁用摄取的状态下执行，结束后自动恢复（`core/src/management/gate.rs:49-64`）。

| 场景 | 门控方式 |
| --- | --- |
| 常规上报 | `with_ingestion`，闸门关闭即丢弃并计数（`core/src/kernel.rs:111-115`） |
| 清空内存 | `with_paused` 包裹 `store.clear()` 与回放重置（`core/src/management/routes.rs:374-379`） |
| 加载会话 | `with_paused` 包裹 `replace_from` 与回放重置（`core/src/management/routes.rs:503-506`） |

会话保存与加载都是异步操作：入队后立刻回 202 与 `operation_id`，状态可轮询（`core/src/management/routes.rs:473`、`:491-529`、`:532-541`）。同一时刻只允许一个持久化操作，重复提交回 409 `operation_busy`（`core/src/management/server.rs:126-137`），已结束的操作记录最多保留 128 条（`core/src/management/server.rs:46`、`:150-152`）。保存前会校验会话名，拒绝符号链接与越出数据根目录的路径（`core/src/management/server.rs:452-497`）。

工作区是单文档覆盖写：`PUT /api/v1/workspace` 校验数值与结构上限后原子写入 `.fly-ruler/workspace.json` 并自增修订号，文档上限 1 MiB、最多 64 张图、每图最多 64 条曲线（`core/src/management/workspace.rs:11-13`、`:260-264`）。写入的 `revision` 用于让控制台判断草稿是否过期。

## 静态托管与配置注入

管理面在 `web_root` 下寻找 `index.html`。找到就校验其中是否含哨兵 `__FLY_RULER_RUNTIME_CONFIG__`，缺失时启动失败并报 `does not contain the runtime configuration placeholder`（`core/src/management/server.rs:401-417`）。

注入的内容是一段 JSON，`api_base_url` 默认取同源 `/api/v1`，`websocket_url` 默认取同源 `/api/v1/ws`，两者都可由配置覆盖；序列化后再把 `<`、`>`、`&` 转成 `\u003c`、`\u003e`、`\u0026`（`core/src/management/server.rs:418-430`）。`/` 与 `/index.html` 返回注入后的页面，其余路径先查静态文件，未命中则回落到同一份页面以支持前端路由（`core/src/management/server.rs:316-347`）。`web_root` 下没有 `index.html` 时只提供 API，其它路径回 404（`core/src/management/server.rs:345-347`、`:404-410`）。

## CORS

CORS 层只放行配置中的来源，方法限 GET、POST、PUT，请求头限 `Content-Type` 与 `Origin`（`core/src/management/server.rs:300-314`）。来源字符串在启动时解析，非法值直接让启动失败（`core/src/management/server.rs:301-310`）。默认来源是 localhost 与 127.0.0.1 的 3000、5173、8000、18003 端口（`core/src/config.rs:217-226`）。

## 启动配置

默认配置文件名是 `fly-ruler-server.toml`，只有当前目录存在该文件时才会读取，也可以用 `--config` 指定其它路径；TOML 顶层分 `transport`、`cursor_stream`、`management`、`playback`、`logging` 五节，未知键会报错（`server/src/config.rs:11`、`:61-70`、`:122-129`）。`schema_version` 必须等于运行配置的版本号，示例文件里的值是 2（`server/src/config.rs:130-136`、`server/fly-ruler-server.example.toml:1`）。

单个配置项的取值顺序是命令行、TOML、内置默认值（`server/src/config.rs:143-206`）。默认 UDP 监听是 `127.0.0.1:18002`，管理面默认 `127.0.0.1:18003`，数据根目录默认 `sessions`，静态目录默认 `web/dist`（`server/src/config.rs:146`、`:154-169`）。`data_root`、`web_root` 与日志文件的相对路径按服务器启动目录解析（`server/src/config.rs:138`、`:296-302`）。`--http` 与 `--no-http` 互斥，决定是否启动管理面，未指定时取 `management.enabled`，默认启用（`server/src/config.rs:37-40`、`:147-153`）。

可用的命令行开关覆盖各个分节：`--udp-listen`、`--http-listen`、`--data-root`、`--web-root`、`--public-api-base-url`、`--public-websocket-url`、`--ws-hz`、`--cors-origin`、`--heartbeat-interval-secs`、`--heartbeat-timeout-secs`、`--cursor-publish-hz`、`--max-subscribers`、`--playback-default-speed`、`--playback-min-speed`、`--playback-max-speed`、`--log-level` 与 `--log-file`（`server/src/config.rs:18-59`）。

启动前会统一校验：心跳间隔大于零且超时大于间隔，发布帧率有限且大于零，订阅上限大于零，WebSocket 帧率有限且大于零，倍速满足 `0 < min_speed <= default_speed <= max_speed`，日志级别是五个枚举值之一（`server/src/config.rs:218-256`）。示例文件里还有 `cursor_stream.reconnect_initial_secs` 与 `reconnect_max_secs`，它们属于客户端重连参数，服务端组装运行配置时只取 `publish_hz` 与 `max_subscribers`，这两个键会被解析但不被使用（`server/src/config.rs:267-271`、`core/src/config.rs:75-84`）。

本地联调用 `just dev server` 只起服务端，`just dev web` 只起控制台开发服务器，`just dev all` 先起服务端再起控制台（`justfile:26-48`）。

## 相关页面

- 架构总览（`docs/dev/01-architecture.md`）：内核、传输与管理面的分工
- [内核分层与并发](kernel-concurrency.md)：共享状态怎么交给管理面、停机顺序与持久化操作队列
- [存储与回放](storage-playback.md)：路由背后的数据模型与回放状态机
- 接口参考（`docs/dev/api.md`）：管理模块的公开类型
- `docs/guide/03-console.md`：配置项与控制台用法
