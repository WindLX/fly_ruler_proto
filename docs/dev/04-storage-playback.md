# 存储与回放

服务端把收到的状态、事件与遥测帧全部留在内存的时间序列 store 里，回放控制器在它之上维护一条全局游标，管理面按时间窗口取数、按游标对齐推送增量（`core/src/store.rs:139`、`core/src/playback.rs:100`）。磁盘上的一次落盘是一个会话目录，由一份 `meta.json` 与三份 Parquet 文件组成。

## 内存组织

`TimeSeriesStore` 是从飞机 ID 到 `AircraftTimeSeries` 的映射，外层用 `RwLock` 包住 `DashMap`，另有一张记录状态到达时刻的 `live_state_receipts`（`core/src/store.rs:139-142`）。

每架飞机的分片含四条流：按时间戳排序的 `states`、`events`、`telemetry`，以及由首条 `Spawn` 事件写入的 `config`（`core/src/store.rs:96-105`）。遥测帧不做自动裁剪（`core/src/store.rs:101`）。

写入统一走 `append_message_with_config`：只接受 `Request` 信封，源时间戳非有限的消息在分派前丢弃（`core/src/store.rs:333-337`），`aircraft_id` 取自消息里的 UUID，缺失时归到 `"unknown"`（`core/src/store.rs:348-352`）。

| 消息 | 结果 |
| --- | --- |
| `StateUpdate` | 保序插入状态序列并刷新实时收包时刻（`core/src/store.rs:359-364`） |
| `Spawn` | 校验遥测流声明，写入初始状态与 `Spawn` 事件，并设置 `config`（`core/src/store.rs:365-374`） |
| `Despawn` | 插入 `Despawn` 事件并清除实时收包时刻（`core/src/store.rs:375-376`） |
| `CustomEvent` | 保序插入事件序列（`core/src/store.rs:377-379`） |
| `TelemetryFrame` | 按 spawn 声明的 schema 校验后保序插入遥测序列（`core/src/store.rs:380-387`） |

保序插入用 `partition_point` 定位（`core/src/store.rs:207-211`），乱序到达的样本会落到正确位置。实时收包时刻用单调时钟记录，新鲜度只看到达时间，与样本自身的时间戳无关（`core/src/store.rs:397-406`）。

## 磁盘布局

一次落盘写进一个目录，`save_to_disk` 依次写 `meta.json`、`states.parquet`、`events.parquet`、`telemetry.parquet`（`core/src/store.rs:795-805`）。

| 文件 | 列 |
| --- | --- |
| `meta.json` | `version` 与按 ID 排序的飞机清单（`core/src/store.rs:825-847`） |
| `states.parquet` | `aircraft_id`、`timestamp`、`state_payload`（`core/src/store.rs:880-882`） |
| `events.parquet` | `aircraft_id`、`timestamp`、`event_type`、`event_payload`（`core/src/store.rs:917-920`） |
| `telemetry.parquet` | `aircraft_id`、`timestamp`、`frame_payload`（`core/src/store.rs:959-961`） |

`meta.json` 每项记录 id、name、toml_config、时间范围与状态/事件条数，不含遥测条数（`core/src/store.rs:825-843`）。加载时按状态、事件、遥测、清单的顺序填充一个新 store，再用 `replace_from` 整体替换现有内容（`core/src/store.rs:808-818`），Parquet 按 1024 行一批读取（`core/src/store.rs:997`）。遥测字段声明由 `Spawn` 事件载荷恢复，清单里没有这项信息。

## 会话目录

默认数据根目录是 `sessions`（`core/src/config.rs:212`），管理面在启动时把它解析为绝对路径并创建（`core/src/management/server.rs:249-250`、`:351-362`），每个子目录是一个具名会话。

保存由 `save_snapshot_atomic` 完成：先写入临时目录 `.{name}.tmp-{operation_id}`，目标不存在就直接改名；目标存在且允许覆盖时先把旧目录改名为 `.{name}.bak-{operation_id}`，改名失败再回滚，成功后删除备份（`core/src/management/server.rs:499-537`）。临时目录与备份目录的存在让保存过程可以中断而不会留下半个会话。

只有含 `meta.json` 的目录会被 `GET /api/v1/sessions` 列为会话，名字以 `.` 开头且含 `.tmp-` 或 `.bak-` 的目录被跳过（`core/src/management/routes.rs:395-402`）。工作区文档存放在数据根目录下的 `.fly-ruler/workspace.json`（`core/src/management/workspace.rs:238-248`）。

## 索引与查询

具名会话不带独立索引文件，`meta.json` 既是最小清单，也是列表与加载的筛选依据。字段级索引按需现算：`GET /api/v1/aircraft/{id}/series/catalog` 依据该机的标准状态字段、稳定推进器字段与已注册遥测字段生成可选系列清单（`core/src/management/series.rs:104-114`、`core/src/management/routes.rs:251-262`）。

按窗口取数有三条路径，返回结构都带 `total`、`offset`、`limit` 与 `items`：

| 端点 | 范围 | 位置 |
| --- | --- | --- |
| `/api/v1/aircraft/{id}/states` | 单机状态样本 | `core/src/management/routes.rs:194-212` |
| `/api/v1/aircraft/{id}/events` | 单机生命周期与自定义事件 | `core/src/management/routes.rs:214-232` |
| `/api/v1/timeline/events` | 全部飞机的事件，带 `aircraft_id` | `core/src/management/routes.rs:234-249` |

分页默认 1000 条、上限 10000 条，越界回 400 `invalid_limit`（`core/src/management/routes.rs:184-189`）。`POST /api/v1/series/query` 在窗口之上做数值抽取与降采样：选择器数量限 1 到 64，`max_points` 限 100 到 20000，请求体拒绝未知字段，返回每条的原始点数、返回点数与统计量（`core/src/management/series.rs:8-12`、`:270-286`）。

## 回放游标与倍速

`PlaybackController` 持有 store、回放配置与内部状态，模式有实时、暂停回放与播放中三种（`core/src/playback.rs:13-22`、`:100-104`）。`snapshot()` 先按当前游标推进再返回 `mode`、`cursor_secs`、`speed`、`bounds` 与 `revision`（`core/src/playback.rs:123-128`、`:347-355`）。

时间边界 `bounds` 取全部飞机在状态、事件与遥测三类数据里的最小与最大时间戳。store 为空时边界为空，所有需要游标的操作都会失败：

| 调用 | 校验 | 行为 |
| --- | --- | --- |
| `live()` | 无 | 回到实时，游标取边界末端（`core/src/playback.rs:131-139`） |
| `pause()` | 需要非空边界 | 停在当前游标并暂停（`core/src/playback.rs:142-151`） |
| `seek(timestamp)` | 时间戳有限，越界钳到区间内 | 跳转并暂停（`core/src/playback.rs:154-165`） |
| `play(speed)` | 速度在 `min_speed..=max_speed` | 进入播放，`None` 沿用当前速度（`core/src/playback.rs:168-189`） |
| `set_speed(speed)` | 同上 | 立即改速并保留游标（`core/src/playback.rs:192-203`） |
| `step(unit, direction, count)` | 步数 1..=100 | 暂停并跳到相邻的全局样本或事件时间戳（`core/src/playback.rs:206-238`） |

空边界时 `pause`、`seek`、`play`、`step` 都返回 `PlaybackError::EmptyStore`，其文案是 `the store contains no timeline data`（`core/src/playback.rs:70-73`）。管理面把 `EmptyStore` 映射成 409，其余回放错误映射成 400，响应体统一是 `playback_error` 与错误文案（`core/src/management/server.rs:595-608`）。

播放中的游标按 `实际耗时 × speed` 推进（`core/src/playback.rs:328-343`），追上边界末端就自动切回暂停并自增修订号。`live`、`pause`、`seek`、`play`、`set_speed`、`step` 与两类重置都会自增 `revision`，游标订阅据此判断基线是否失效（`core/src/playback.rs:308-345`、`:241-259`）。

## 游标流式订阅

游标订阅面向控制台与外部消费者：订阅者先以 `CursorSubscriber` 角色握手，再发 `CursorSubscribe` 声明期望帧率，帧率非有限或不大于零会被拒绝（`core/src/transport/server.rs:471-490`）。

发布按 `1 / publish_hz` 的固定节拍进行，落后时跳过错过的 tick 而不补发（`core/src/cursor.rs:152-154`）。每个节拍取当前全部已订阅会话并按地址排序，超出 `max_subscribers` 的部分会被移除订阅（`core/src/cursor.rs:159-172`）；单个订阅者的实际帧率取请求帧率与 `publish_hz` 的较小值（`core/src/cursor.rs:299-300`）。

发布内容分基线与增量：订阅后的第一帧、`revision` 变化或游标回退时发基线快照，其余节拍只发新增事件（`core/src/cursor.rs:192-201`）。事件按 32 条一页切分，序号由批次号与批内下标合成，批次带服务器 epoch；订阅者用 epoch 匹配与 `sequence + 1` 严格递增确认（`core/src/cursor.rs:203`、`core/src/transport/server.rs:208-227`）。未确认的批次按固定间隔重传，超过重试上限后丢弃（`core/src/cursor.rs:276-293`）。

| 参数 | 默认值 | 作用 |
| --- | --- | --- |
| `publish_hz` | 30.0 | 发布节拍，也是订阅者帧率的上限（`core/src/cursor.rs:42-52`） |
| `max_subscribers` | 16 | 每个节拍保留的订阅者数量上限（`core/src/cursor.rs:42-52`） |
| `stale_timeout` | 500 ms | 订阅者失联多久后被重置（`core/src/cursor.rs:42-52`） |
| `event_retry_interval` | 250 ms | 未确认批次的重传间隔（`core/src/cursor.rs:42-52`） |
| `event_retry_limit` | 8 | 单个批次的最大重传次数（`core/src/cursor.rs:42-52`） |

启动时会校验 `publish_hz` 有限且大于零、`max_subscribers` 大于零（`core/src/cursor.rs:136-145`）。

## 保留与回收

store 不对遥测帧做自动裁剪（`core/src/store.rs:101`），配置里的 `StoreConfig` 目前没有任何字段（`core/src/config.rs:185-186`），运行期没有按时间或容量淘汰数据的行为，内存占用与单次会话快照都随数据量线性增长。回收手段只有两个：`POST /api/v1/memory/clear` 清空内存，或者保存一个内容更少的会话再加载回来。

`POST /api/v1/memory/clear` 要求请求体带 `{"confirm": true}`，有持久化操作在跑时回 409 `operation_busy`；实际清空在暂停摄取期间完成，随后回放游标被重置为空（`core/src/management/routes.rs:363-382`）。清空会同时丢弃实时收包时刻，因此清空之后所有飞机都按未生成处理（`core/src/store.rs:789-792`）。

## 相关页面

- [架构与分层](/dev/components/proto/01-architecture)：store 与回放在 core 中的位置
- [回放与时间轴](/guide/components/proto/04-playback)：控制台里的回放操作与时间轴
- [服务与控制台](/guide/components/proto/05-server-and-console)：会话保存与数据根目录的用法
- [接口参考](/dev/components/proto/api)：`TimeSeriesStore` 与 `PlaybackController` 的公开方法
