# 内核分层与并发

这一页面向协议实现者与二次开发者，讲 `fly_ruler_proto_core` 内部的状态归属、并发边界与后台任务，只用 Python client 的用户请读使用手册 `docs/guide/01-install.md`。

内核把全部长期状态收在 `KernelRuntime` 上，它是 `fly-ruler-server`、MSFS 桥与 Godot 绑定共用的内嵌入口（`core/src/kernel.rs:42-51`）：`store` 是 `Arc<TimeSeriesStore>`（`core/src/kernel.rs:43`），`playback` 是 `Arc<PlaybackController>`（`core/src/kernel.rs:45`），`ingestion` 是 `Arc<IngestionGate>`（`core/src/kernel.rs:46`），`sessions` 是 `Arc<RwLock<Option<SessionHandle>>>`（`core/src/kernel.rs:47`），三个运行期句柄 `udp_runtime`、`cursor_runtime`、`management_runtime` 各自可空（`core/src/kernel.rs:48-50`）。

## 组装与句柄

`KernelRuntime::with_config` 先初始化日志，再用同一份 store 引用构造 `PlaybackController`，并新建空闸门与空会话槽位（`core/src/kernel.rs:60-77`）；`new` 只是用默认配置转调它（`core/src/kernel.rs:55-57`）。

`config()`、`store()`、`playback()`、`ingestion()` 克隆引用返回，调用方拿到后可独立持有，内核重启 UDP 或管理面不会让这些句柄失效（`core/src/kernel.rs:80-97`）。

## 启动与停止

`start_server` 先停掉已有 UDP 运行时，再调 `ServerRuntime::start` 绑定端口，把 store、摄取闸门与 store 配置搬进一个同步闭包，闭包体内是 `ingestion.with_ingestion(|| store.append_message_with_config(msg, &store_config))`（`core/src/kernel.rs:102-116`）。

紧接着 `start_server` 启动游标流运行时，把发布者句柄、store、回放控制器与游标配置交给它，最后把 `SessionHandle` 写进会话槽位并保存两个运行时句柄（`core/src/kernel.rs:118-128`）。

`start_management_server` 把 store、playback、ingestion 与会话槽位交给 `ManagementServerRuntime::start`（`core/src/kernel.rs:152-167`）；`stop_server` 先停游标任务再停 UDP 接收循环并清空会话槽位（`core/src/kernel.rs:134-147`），`stop_management_server` 只停管理面（`core/src/kernel.rs:170-174`）。

`active_sessions` 与 `udp_local_addr` 都直接转发到 UDP 运行时，未启动时返回空列表或 `udp server is not running`（`core/src/kernel.rs:189-206`）。

## 状态归属与并发边界

| 状态 | 持有者 | 同步原语 | 写入者 |
| --- | --- | --- | --- |
| 时间序列数据 | 内核 store | `RwLock<DashMap<AircraftId, AircraftTimeSeries>>`（`core/src/store.rs:140`） | UDP 接收闭包、会话加载、清空 |
| 实时收包时刻 | 内核 store | `DashMap<AircraftId, Instant>`（`core/src/store.rs:141`） | `append_message_with_config` |
| 回放状态 | 回放控制器 | `Mutex<PlaybackInner>`（`core/src/playback.rs:103`） | HTTP 回放路由与游标发布任务 |
| 摄取许可 | `IngestionGate` | `AtomicBool` + `Mutex` + `RwLock`（`core/src/management/gate.rs:5-10`） | 维护窗口 |
| UDP 会话表 | 传输层 | 两张 `Arc<Mutex<HashMap<..>>>`（`core/src/transport/server.rs:104-108`） | 握手与心跳接收循环 |
| 持久化操作 | 管理面 | `AtomicBool` + `Mutex` + `broadcast` + `Notify`（`core/src/management/server.rs:106-113`） | 会话保存与加载路由 |
| 工作区文档 | 管理面 | `Arc<WorkspaceStore>`（`core/src/management/server.rs:101`） | 工作区 PUT |

存储外层 `RwLock` 保护整张 `DashMap`，内层按飞机 ID 分片，因此不同飞机的追加可以并发，`replace_from` 与 `clear` 需要整表写锁并同时清掉实时时刻表（`core/src/store.rs:774-793`）。

实时收包时刻表与时间序列分开存放，`live_state_age` 只读它来算单调年龄，因此从磁盘加载出来的会话没有实时时刻，会被直接判为陈旧（`core/src/store.rs:397-405`）。

`PlaybackController` 自己持有 store 引用、回放配置与那把 `Mutex<PlaybackInner>`；`snapshot` 先取 store 的活动时间范围再上锁推进有效游标，所以 HTTP、WebSocket、游标发布任务与渲染端看到的是同一份状态（`core/src/playback.rs:100-104`、`core/src/playback.rs:123-128`）。

## 摄取闸门

`IngestionGate` 用 `AtomicBool` 表示开关、`AtomicU64` 记丢弃数、`Mutex` 串行化维护窗口、`RwLock` 表达“写入进行中”（`core/src/management/gate.rs:5-10`）。

`with_ingestion` 取共享读许可后执行一次追加；闸门关闭时只累加丢弃计数并返回 `None`，许可前后各读一次开关是为了避免与维护窗口交错（`core/src/management/gate.rs:30-46`）。

`with_paused` 先抢维护 `Mutex`、把开关置假，再取写许可，退出时由 `ResumeIngestion` 的 `Drop` 恢复开关（`core/src/management/gate.rs:49-64`、`core/src/management/gate.rs:77-89`）；`dropped_count()` 与 `is_enabled()` 供状态接口读取（`core/src/management/gate.rs:67-74`）。

## 会话表与过期

`Session` 同时记墙上时间 `last_seen_secs` 与单调时刻 `last_seen_at`，过期判断只看单调值，因此改系统时钟不会误杀会话（`core/src/transport/server.rs:21-39`、`core/src/transport/server.rs:58-65`）。

`SessionState` 维护地址到会话与客户端 UUID 到地址两张表（`core/src/transport/server.rs:104-108`）；`set_session` 在同一把锁里完成“同一 UUID 换地址就删掉旧地址并插入新会话”（`core/src/transport/server.rs:171-193`）；`touch_addr` 只刷新已存在会话，未握手的地址不会被建表（`core/src/transport/server.rs:163-169`）。

`cleanup_expired` 在每次收包前按超时清掉两张表里的记录并记日志 `session expired and removed`（`core/src/transport/server.rs:258-289`）；超时取自 `TransportConfig::heartbeat_timeout_secs`，绑定 socket 时取 `max(1)` 秒（`core/src/transport/server.rs:301-309`）。

## 接收循环与事件队列

`ServerRuntime::start` 启动时把闭包装进 `Arc<dyn Fn(pb::Message, SocketAddr) + Send + Sync>`，再用 `tokio::spawn` 跑接收循环（`core/src/transport/server.rs:411-427`）。

闭包是同步的，文档注释明确要求不能阻塞，同步 I/O 或重计算都会卡住 UDP 接收循环（`core/src/transport/server.rs:394-398`、`core/src/transport/server.rs:406-410`）。

循环用 `tokio::select!` 在停机令牌与 `recv_from` 之间选择；握手校验 `hs.version == PROTOCOL_VERSION`，通过后写入会话并回 ACK，心跳只回 ACK（`core/src/transport/server.rs:445-470`）。

游标订阅先校验 `requested_hz` 有限且大于零，再要求该地址已经是完成握手的订阅者，否则回 `cursor subscription requires a subscriber handshake`（`core/src/transport/server.rs:471-491`）；游标事件确认只更新会话记录、不产生回包（`core/src/transport/server.rs:492-499`）。

只有 `Producer` 角色的会话能上传 `AircraftEvent`，否则回 `only producer sessions may upload aircraft events`（`core/src/transport/server.rs:501-512`）；客户端发来的 `Response` 与 `ServerPush` 被直接忽略（`core/src/transport/server.rs:515-521`）。

通过校验的飞机事件先进入循环局部的 `pending_events: VecDeque`，随后在同一个 `select!` 分支里被逐个同步交给闭包，队列只用于解耦借用，不跨任务传递（`core/src/transport/server.rs:429`、`core/src/transport/server.rs:532-547`）。

停机时 `stop` 取消令牌、等待接收任务结束并关闭 socket（`core/src/transport/server.rs:596-601`）。

## 后台任务与通道

游标发布是一个独立后台任务：`CursorStreamRuntime::start` 校验 `publish_hz` 与 `max_subscribers` 后 `tokio::spawn`，用 `tokio::time::interval` 定拍并把错过的节拍设为 `Skip`，因此积压时丢补拍而不是追赶（`core/src/cursor.rs:129-158`）。

每拍它从会话表里筛出已订阅的 `CursorSubscriber`，按地址排序，超出 `max_subscribers` 的会话被 `remove_session` 摘除（`core/src/cursor.rs:161-172`）。

发布前先取一次 `playback.snapshot()` 并自增帧序号，再按每个订阅者已确认的事件序列决定发基线事件还是增量事件（`core/src/cursor.rs:175-200`、`core/src/cursor.rs:829`、`core/src/cursor.rs:871`）；`stop` 取消令牌并等待任务退出（`core/src/cursor.rs:339-345`）。

客户端侧的 `CursorClient` 是只收不写的 UDP 客户端，`connect` 绑定随机本地端口并要求 `requested_hz` 有限且大于零（`core/src/cursor.rs:529`、`core/src/cursor.rs:543-550`）。

管理服务同样跑在 `tokio::spawn` 里：`ManagementServerRuntime::start` 校验配置、解析并创建数据根目录、绑定 TCP、装配 `AppState` 与路由，再用 `axum::serve` 配合取消令牌做优雅停机（`core/src/management/server.rs:240-283`）。

停机顺序是先取消并等 HTTP 任务结束，再等持久化操作空闲（`core/src/management/server.rs:291-297`）；`wait_idle` 用 `Notify` 等待当前操作收尾，避免关库时还有写盘在半途（`core/src/management/server.rs:219-227`）。

`OperationManager` 用 `AtomicBool` 保证同一时刻只有一个持久化操作，`Mutex` 保护记录队列，容量 64 的 `broadcast::Sender<Value>` 推送 `operation_status` 消息（`core/src/management/server.rs:106-125`）。重复提交时 `begin` 回 409 `operation_busy`，已结束的记录最多保留 `MAX_OPERATIONS = 128` 条（`core/src/management/server.rs:127-155`、`core/src/management/server.rs:46`）。

## 一致性快照

`save_session` 在 `with_paused` 窗口里克隆 store 快照再落盘；`load_session` 先在旁路 store 上读盘，再在同一个窗口里 `replace_from` 并重置回放；`clear_session` 在同一窗口里清空 store 与回放（`core/src/kernel.rs:209-234`）。

Python 绑定自己持有一个 tokio 运行时并把网络操作 `block_on` 到同步 API 上，因此 Python 侧调用会阻塞当前线程，关闭后的调用得到 `ConnectionError`（`bindings/python/src/client.rs:19-21`、`bindings/python/src/client.rs:341`）。

## 相关页面

- 架构总览（`docs/dev/01-architecture.md`）：分层、构件与数据路径
- [UDP 会话与可靠性](udp-session.md)：握手、心跳、ACK 与游标分片的重传规则
- [存储与回放](storage-playback.md)：store 与回放控制器的公开接口
- [管理服务与 HTTP/WS 路由](management-api.md)：这些共享状态如何暴露成 REST 与推送
- Python 绑定（`docs/dev/02-python-binding.md`）：内核在本机进程内被嵌入时的生命周期
