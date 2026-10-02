# 回放与游标

回放游标是服务端的全局时间线状态，不属于单条 UDP 会话：Python 绑定只负责把状态、事件与遥测写进存储，读取哪一时刻的数据由管理接口上的游标决定。

游标由服务端的回放控制器持有（`core/src/playback.rs:90-97`），下面所有命令都发往管理 HTTP 端口，默认 `127.0.0.1:18003`。

## 读取当前游标

```bash
curl -s http://127.0.0.1:18003/api/v1/playback
```

返回一个回放快照，字段是 `mode`、`cursor_secs`、`speed`、`bounds`、`revision`（`core/src/playback.rs:44-57`）：

```json
{
  "mode": "live",
  "cursor_secs": 42.5,
  "speed": 1.0,
  "bounds": [0.0, 42.5],
  "revision": 7
}
```

`mode` 取值 `live`、`replay_paused`、`replay_playing`（`core/src/playback.rs:13-22`）。`cursor_secs` 是当前的全局游标，存储为空时为 `null`；`bounds` 是当前数据的时间范围；`revision` 每次显式改变时间线都会加一，便于轮询判断状态是否变化（`core/src/playback.rs:347-355`）。同一个快照也出现在 `GET /api/v1/status` 的 `playback` 字段里（`core/src/management/routes.rs:83-104`）。

## 切换模式与推进游标

| 请求 | 作用 |
| --- | --- |
| `POST /api/v1/playback/live` | 回到实时模式，游标跟随最新数据（`core/src/playback.rs:131-139`） |
| `POST /api/v1/playback/pause` | 停在当前游标，转成 `replay_paused`（`core/src/playback.rs:142-151`） |
| `POST /api/v1/playback/play` | 从当前游标开始播放，body 为 `{"speed": 2.0}` 或 `{}`（`core/src/playback.rs:168-189`） |
| `PUT /api/v1/playback/speed` | 只改倍速，body 为 `{"speed": 4.0}`（`core/src/playback.rs:192-203`） |
| `POST /api/v1/playback/seek` | 跳到指定时刻，body 为 `{"timestamp": 30.0}`（`core/src/playback.rs:154-165`） |
| `POST /api/v1/playback/step` | 按样本或事件步进，body 为 `{"unit":"sample","direction":"next","count":1}`（`core/src/playback.rs:206-238`） |

```bash
curl -s -X POST http://127.0.0.1:18003/api/v1/playback/seek \
  -H 'Content-Type: application/json' \
  -d '{"timestamp": 30.0}'
```

`seek` 会把游标截断到 `bounds` 范围内并转入 `replay_paused`（`core/src/playback.rs:154-165`）；`step` 的 `count` 必须在 1 到 100 之间，`previous` 方向到边界时游标停在原地，`revision` 仍会增加（`core/src/playback.rs:206-238`，`core/src/playback.rs:495-511`）。

## 播放的时间轴语义

播放状态下游标由单调时钟推进：`cursor = 起点 + 已流逝墙钟时间 × speed`（`core/src/playback.rs:308-345`）。这个锚点与源时间轴的时间戳基准无关，所以产出方用 Unix 时间还是仿真时钟都不影响播放速度。

游标推进到 `bounds` 末端时自动落回 `replay_paused`，并把 `revision` 加一（`core/src/playback.rs:308-345`）。`live` 模式下每次读快照都会把游标对齐到当前数据末端。

出错时返回的 `code` 是 `playback_error`：存储为空返回 409，时间戳非有限、倍速越界或步长越界返回 400（`core/src/management/server.rs:595-607`）。倍速的合法范围来自配置项的 `min_speed` 到 `max_speed`（`server/fly-ruler-server.example.toml:29-31`），默认 0.1 到 16.0（`server/src/config.rs:239-250`）。

## 按游标读取数据

不指定时刻时，单架飞机的状态按当前游标解析：`live` 取最新收到的状态，回放模式取不晚于游标的最近样本（`core/src/playback.rs:262-289`，`core/src/management/routes.rs:154-159`）。

```bash
curl -s "http://127.0.0.1:18003/api/v1/aircraft/<aircraft_uuid>/state"
curl -s "http://127.0.0.1:18003/api/v1/aircraft/<aircraft_uuid>/state?at=30.0"
```

响应包含 `spawned` 标志，它在回放时按游标判断这架飞机当时是否存在（`core/src/store.rs:436-461`）。

时间范围与分页查询用 `states`、`events` 与 `timeline/events` 三个端点：

```bash
BASE=http://127.0.0.1:18003/api/v1
curl -s "$BASE/aircraft/<aircraft_uuid>/states?start=0&end=60&offset=0&limit=100"
curl -s "$BASE/timeline/events?start=0&end=60&offset=0&limit=100"
```

`states` 与 `events` 需要有限且 `start <= end` 的范围，`limit` 必须落在 1 到 10000 之间（`core/src/management/routes.rs:169-232`）。数值曲线用 `POST /api/v1/series/query`，选择器写法见 [遥测与时间序列](/guide/components/proto/03-telemetry)。

## 会话持久化

磁盘上的会话目录可以整体保存与装载，加载完成后游标随之重置：

```bash
curl -s http://127.0.0.1:18003/api/v1/sessions
curl -s -X POST http://127.0.0.1:18003/api/v1/sessions/night-flight/save \
  -H 'Content-Type: application/json' -d '{"overwrite": true}'
curl -s -X POST http://127.0.0.1:18003/api/v1/sessions/night-flight/load
```

保存与加载都是异步操作，成功受理返回 202 与一个 `operation_id`，用 `GET /api/v1/operations/<id>` 查询进度（`core/src/management/routes.rs:417-541`）。目标目录已存在且未指定 `overwrite` 时保存返回 409 `session_exists`，加载不存在的会话返回 404 `session_not_found`。

加载完成后游标被重置为 `replay_paused` 并停在数据的起始时刻（`core/src/playback.rs:251-259`，`core/src/management/routes.rs:505`）。

清空内存里的时间线需要显式确认：

```bash
curl -s -X POST http://127.0.0.1:18003/api/v1/memory/clear \
  -H 'Content-Type: application/json' -d '{"confirm": true}'
```

`confirm` 不为 true 时返回 400 `confirmation_required`；清空后游标变为 `null`、模式为 `replay_paused`（`core/src/management/routes.rs:363-382`，`core/src/playback.rs:241-248`）。

## 与 Web 控制台的关系

控制台用的就是上面这套 REST 端点，路径都由 `/api/v1` 前缀拼出（`web/src/runtime.ts:7-8`）：`web/src/api.ts:50-69` 依次调用 `/playback`、`/playback/live`、`/playback/pause`、`/playback/play`、`/playback/seek`、`/playback/step`、`/playback/speed`，时间轴与图表分别取 `timeline/events` 与 `series/query`（`web/src/api.ts:48`，`web/src/api.ts:98`）。

界面上的连续刷新来自 WebSocket `/api/v1/ws`，它按配置的 `websocket_hz` 周期推送当前快照（`core/src/management/routes.rs:606-686`）。这个通道是只读的：往里发消息会收到 `websocket_read_only` 错误，明确要求播放命令走 REST（`core/src/management/routes.rs:637-646`）。

配套的可运行示例是 `bindings/python/examples/06_circle_demo.py`；示例清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [遥测与时间序列](/guide/components/proto/03-telemetry)
- [存储与回放](/dev/components/proto/04-storage-playback)
- [服务与管理接口](/dev/components/proto/05-server-http-ws)
- [服务与控制台](/guide/components/proto/05-server-and-console)
- [协议 API 参考](/api/proto/)
