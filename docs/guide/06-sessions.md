# 会话、回放与导出

## 数据落在哪

服务端把一段运行记录组织成一个会话，会话在磁盘上的位置是 `data_root` 下的同名目录，写盘目标是 `state.config.data_root.join(&name)`（`core/src/management/routes.rs:430`）。

`data_root` 默认取 `sessions`（`server/src/config.rs:158-163`），MSFS 桥的默认值相同（`bindings/msfs/src/config.rs:212-223`）。因此在默认配置下，名为 `flight-test` 的会话位于运行目录的 `sessions/flight-test/`。

会话目录里是 `meta.json` 加上按数据类型分文件的记录，写出顺序是 `write_meta`、`write_states_parquet`、`write_events_parquet`、`write_telemetry_parquet`（`core/src/store.rs:795-805`）。

`meta.json` 只保存协议版本与飞机清单，不含逐点数据。每架飞机记录 `id`、`name`、`toml_config`、`time_range`、`state_count` 与 `event_count`，清单按飞机 ID 排序（`core/src/store.rs:820-847`，排序见 `:838`），最终以 `MetaFile { version: PROTOCOL_VERSION.to_string(), aircrafts }` 写入（`core/src/store.rs:839-842`）。

某类记录为空时不会留下对应文件，例如只跑过状态与事件的会话目录里只有 `meta.json`、`states.parquet` 与 `events.parquet`。

会话名在保存前会经过校验，只接受能作为目录名的合法片段（`core/src/management/routes.rs:423-476`）；名字里带路径分隔符或保留前缀会被拒绝，避免写盘越出 `data_root`。

## 保存、加载与清空

会话的增删查改都走管理面，接口挂在 `/api/v1` 前缀下（`core/src/management/routes.rs:33-57`）。

`GET /api/v1/sessions` 列出 `data_root` 下的会话，跳过以 `.tmp-`、`.bak-` 开头的临时目录与没有 `meta.json` 的目录，结果按名字排序（`core/src/management/routes.rs:384-415`）。

`POST /api/v1/sessions/{name}/save` 立即返回 `202 {"operation_id": "..."}`，真正的写盘在后台任务里用原子快照完成（`core/src/management/routes.rs:423-476`，返回见 `:472-475`）。目标目录已经存在且请求没有声明覆盖时返回 `409 session_exists`。拿返回的 operation id 可以查 `GET /api/v1/operations/{id}` 跟踪进度。

`POST /api/v1/sessions/{name}/load` 同样返回 `202`，加载完成后用磁盘数据替换内存中的时间序列，并把回放游标重置到时间范围起点、模式置为 `replay_paused`（`core/src/management/routes.rs:478-530`，返回见 `:526-529`；游标重置见 `core/src/playback.rs:251-259`）。

`POST /api/v1/memory/clear` 需要请求体带 `{"confirm": true}`，否则返回 `400 confirmation_required`；成功后清空内存中的记录并复位回放状态（`core/src/management/routes.rs:363-382`）。

## 回放

回放控制的路由是 `POST /api/v1/playback/live|pause|play|seek|step` 与 `PUT /api/v1/playback/speed`（`core/src/management/routes.rs:33-57`）。

回放状态快照包含 `mode`、`cursor_secs`、`speed`、`bounds` 与 `revision`（`core/src/playback.rs:44-57`），其中 `mode` 取 `live`、`replay_paused` 或 `replay_playing`（`core/src/playback.rs:13-22`）。

`live` 把游标放到时间范围末端并回到实时跟随（`core/src/playback.rs:131-139`），`pause` 冻结当前游标（`core/src/playback.rs:142-151`），`seek` 把游标夹到 `bounds` 内并转入暂停（`core/src/playback.rs:154-165`），`play` 从当前游标继续播放并可顺带改倍速（`core/src/playback.rs:168-189`），`speed` 单独设置倍速（`core/src/playback.rs:192-203`），`step` 按样本或事件前后步进，单次 `count` 在 1 到 100 之间（`core/src/playback.rs:206-238`）。

控制台顶部工具栏提供 `Live`、`Pause`、`Play` 三个按钮和一个倍速下拉框，右侧的读数显示相对游标时间、模式与绝对时间（`web/src/components/PlaybackToolbar.vue:73-106`）。倍速下拉的取值是 `0.1`、`0.25`、`0.5`、`1`、`2`、`4`、`8`、`16`（`web/src/components/PlaybackToolbar.vue:92-96`）。

控制台还注册了键盘快捷键：`Space` 在暂停与播放之间切换，`←`/`→` 按样本步进（按住 `Shift` 一次走 10 个），`↑`/`↓` 按事件步进，`Home`/`End` 跳到时间范围两端（`web/src/shortcuts.ts:11-29`）。

底部时间轴在轨道上拖动即可 seek，滚轮缩放可见区间，事件簇可以直接点击跳到对应时间（`web/src/components/TimelineBar.vue:167-179`、`:183`、`:221`、`:335-336`、`:363-364`）。

倍速的可用范围由服务端配置决定，示例配置给出的边界是 `min_speed = 0.1`、`default_speed = 1.0`、`max_speed = 16.0`（`server/fly-ruler-server.example.toml:28-31`），加载时校验 `0 < min <= default <= max`（`server/src/config.rs:239-250`）。

管理面另外提供 `/api/v1/ws`，以 WebSocket 推送只读的回放与状态更新，适合自己写监控面板时替代轮询（`core/src/management/routes.rs:33-57`）。

快照里的 `revision` 随回放状态变化递增，用来判断两份快照是否属于同一个版本；`bounds` 是整个会话可选区间，控制台据此换算相对时间（`core/src/playback.rs:44-57`）。

## 导出

会话目录本身就是导出物，把整个目录拷走即可，`meta.json` 与各 `*.parquet` 都是自包含的，换一台机器也能读。

图表布局、面板开关、主题与查询区间这类界面状态存放在工作区里，通过 `GET /api/v1/workspace` 与 `PUT /api/v1/workspace` 读写（`core/src/management/routes.rs:33-57`）。

工作区持久化在 `{data_root}/.fly-ruler/workspace.json`（`core/src/management/workspace.rs:237-239`），随 `data_root` 一起迁移就能带走。

parquet 按数据类型分表存放，加载时依次读回 states、events、telemetry 与 meta 四部分（`core/src/store.rs:807-818`），单张表损坏只会影响对应类型的数据。

## 示例

`bindings/python/examples/04_sessions_and_playback.py` 是配套示例：一架飞机在水平面内做匀速圆周飞行，按 `--hz`（默认 30）持续上报状态，并按 `--event-every` 周期发送 `demo.tick` 事件、结束时发送 `demo.finished`。

`--duration 0` 让它一直跑到中断，产生的数据适合在控制台里暂停、拖动时间轴并在事件标记之间步进。

保存这个会话可以在控制台里触发，也可以直接调用 `POST /api/v1/sessions/{name}/save`，然后通过 `POST /api/v1/sessions/{name}/load` 在之后重现。

示例目录里的 `README.md` 给出了每个脚本的场景与运行前提，需要哪类数据就挑对应脚本跑。

多机场景可以一次注册多架飞机，它们写进同一个会话目录但保留各自的 `id`，控制台在目录里按飞机切换字段。

## 相关页面

- [控制台](03-console.md)
- [遥测与图表](05-telemetry.md)
- [无 MSFS 时用独立服务端](08-standalone-server.md)
