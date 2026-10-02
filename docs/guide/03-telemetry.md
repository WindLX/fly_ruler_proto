# 遥测与时间序列

除了位置、速度这类固定字段的状态，产出方还可以在上报时声明任意数量的遥测流，按流发送标量样本。

遥测与状态走同一条 UDP 通道和同一份时间序列存储：声明在 spawn 时注册，样本按时间戳保序落库，读取统一走管理接口的序列查询。

## 声明遥测流

遥测流在构造客户端时随 spawn 一起注册：

```python
from fly_ruler_proto_python import (
    FlyRulerClient,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
)

schema = TelemetryStreamSchema(
    stream_id="engine",
    name="发动机遥测",
    nominal_rate_hz=10.0,
    fields=[
        TelemetryField(
            "rpm",
            label="转速",
            unit="rpm",
            value_type=TelemetryValueType.I64,
        ),
        TelemetryField(
            "egt",
            label="排气温度",
            unit="degC",
            value_type=TelemetryValueType.F64,
        ),
        TelemetryField("afterburner", group="state", value_type=TelemetryValueType.Bool),
    ],
)

with FlyRulerClient(
    "127.0.0.1:18002",
    "F-16",
    telemetry_schemas=(schema,),
) as aircraft:
    ...
```

`TelemetryField(field_id, label, group, unit, description, value_type)` 的后四个参数默认空字符串，`value_type` 默认 `TelemetryValueType.F64`（`bindings/python/src/protocol.rs:364`），`TelemetryStreamSchema(stream_id, fields, name, nominal_rate_hz)` 的 `name` 默认空、`nominal_rate_hz` 默认为 `None`（`bindings/python/src/protocol.rs:408`）。

字段类型只有三种：`TelemetryValueType.F64`、`TelemetryValueType.I64`、`TelemetryValueType.Bool`，分别对应 wire schema 里的 `F64=1`、`I64=2`、`BOOL=3`（`bindings/python/src/protocol.rs:317-324`，`core/proto/fly_ruler.proto:71-76`），值为 `0` 的 `UNSPECIFIED` 不能用于字段。

注册时客户端会做一轮校验：`stream_id` 不能为空且不能重复，`field_id` 不能为空且在同一流内唯一，`nominal_rate_hz` 若给定则必须是有限正数，任何一条不满足都抛 `ValueError`（`bindings/python/src/client.rs:94-125`）。

服务端还会在收到 spawn 时再校验一次（`core/src/store.rs:1072-1110`）。schema 非法的 spawn 会被整体忽略，只留一条 warn 日志（`core/src/store.rs:366-370`）。

## 发布样本

```python
aircraft.publish_telemetry("engine", (9000, 640.5, False))
aircraft.publish_telemetry("engine", (9000, 640.5, False), timestamp=12.5)
```

`values` 是长度与字段数严格相等的序列，按字段声明顺序逐项对应；长度不符抛 `ValueError`，未知的 `stream_id` 抛 `KeyError`（`bindings/python/src/client.rs:214-223`）。

每个值按该字段声明的类型转换：`F64` 需要可转成浮点，`I64` 需要整数（布尔值会被拒绝），`Bool` 需要布尔值，类型不符抛 `TypeError`（`bindings/python/src/client.rs:303-338`）。

客户端为每条流维护一个从 1 开始递增的序列号，写进 `TelemetryFrame.sequence`（`bindings/python/src/client.rs:229-238`，`core/proto/fly_ruler.proto:102-106`）。服务端不按序列号排序或去重，序列号只用于消费侧识别缺口。

`timestamp` 的语义与 `update_state` 相同，见 [客户端连接与状态更新](/guide/components/proto/02-client-connection)。

## 服务端如何存储

样本经 UDP 到达后由存储层统一分发：遥测帧走 `append_telemetry`（`core/src/store.rs:383-389`，`core/src/store.rs:275`）。

写入前逐项校验：`stream_id` 必须是该飞机在 spawn 时注册过的流，值个数必须等于字段数，每个值的类型必须与声明精确匹配；不匹配则整帧丢弃并记 warn 日志（`core/src/store.rs:1112-1160`，`core/src/store.rs:295-311`）。

遥测帧与状态一样按时间戳保序：时间戳不早于最后一个样本就追加，否则二分插入（`core/src/store.rs:298-311`）。相同时间戳的样本按到达顺序排在一起，不会互相覆盖。

遥测帧不做自动裁剪，会话期间一直保留（`core/src/store.rs:96-105`）。

保存会话时遥测写进独立的 `telemetry.parquet`（`core/src/store.rs:802`，`core/src/store.rs:982`），加载会话时读回并重放（`core/src/store.rs:813`）。

## 查询遥测

管理接口没有直接列出原始遥测帧的端点，读数值统一走序列查询。先看某一架飞机可选的字段：

```bash
curl -s http://127.0.0.1:18003/api/v1/aircraft/<aircraft_uuid>/series/catalog
```

返回里的每个条目带一个选择器，遥测字段的选择器形如 `{"kind":"telemetry","stream_id":"engine","field_id":"rpm"}`（`core/src/management/routes.rs:251-262`，`core/src/management/series.rs:45-52`）。

选择器来自 spawn 时注册的 schema，所以样本还没到达时字段已经出现在目录里（`core/src/management/series.rs:225`）。

拿目录里的选择器查数值：

```bash
curl -s -X POST http://127.0.0.1:18003/api/v1/series/query \
  -H 'Content-Type: application/json' \
  -d '{
    "selections": [
      {
        "aircraft_id": "<aircraft_uuid>",
        "selector": {"kind": "telemetry", "stream_id": "engine", "field_id": "rpm"}
      }
    ],
    "time_range": {"start": 0.0, "end": 60.0},
    "max_points": 500
  }'
```

请求体字段是 `selections`、可选的 `time_range`（闭区间）与可选的 `max_points`（`core/src/management/series.rs:136-145`）。`max_points` 允许 100 到 20000，默认 2000；一次最多 64 个选择器（`core/src/management/series.rs:8-11`）。

响应里每条序列带 `key`、`aircraft_id`、`selector`、`points`（`[时间戳, 数值]` 数组）、`total_points`、`returned_points` 与 `stats`（`core/src/management/series.rs:163-179`）。

数值映射是：`F64` 取原值，`I64` 转成 `f64`，`Bool` 转成 `0` 或 `1`（`core/src/management/series.rs:621-625`）。

选择器指向未注册的字段时返回 `series_error`，消息形如 `unknown telemetry field: engine:rpm`（`core/src/management/series.rs:205-206`）；飞机 id 不存在则是 404（`core/src/management/server.rs:611-620`）。

## best-effort 下丢包与迟到

遥测帧和状态一样是一次性 UDP 数据报，客户端发送后不等 ACK、不重传（`core/src/transport/client.rs:555-594`），所以丢包表现为 `points` 里出现时间缺口，缺口位置只能靠帧里的序列号推断。

迟到样本不会丢：时间戳较旧时服务端插入到历史位置（`core/src/store.rs:298-311`），因此同一时间窗在稍后重查可能返回更多点。

按闭区间查询时，边界上的样本包含在结果里（`core/src/store.rs:491-510`）。

类型或流声明不匹配的帧在服务端静默丢弃，客户端不会收到任何错误（`core/src/store.rs:1112-1160`），排查时看服务端日志里的 `ignored invalid telemetry frame`（`core/src/store.rs:385`）。

配套的可运行示例是 `bindings/python/examples/04_telemetry.py` 与 `bindings/python/examples/05_multi_aircraft.py`；示例清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [客户端连接与状态更新](/guide/components/proto/02-client-connection)
- [回放与游标](/guide/components/proto/04-playback)
- [wire schema](/dev/components/proto/02-wire-schema)
- [服务与管理接口](/dev/components/proto/05-server-http-ws)
- [协议 API 参考](/api/proto/)
