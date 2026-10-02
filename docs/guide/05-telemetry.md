# 遥测与图表

## 遥测流是什么

飞机状态与标准事件之外，协议还留了一条自定义数据通道，用来传发动机温度、转速、燃油量、控制律内部状态或执行器行程这类连续量。

这条通道以遥测流为单位组织。每个流有独立的标识、显示名、期望频率和一组字段，客户端在建立连接时声明结构，之后按声明的字段顺序发数据。

遥测数据进入服务端后与状态、事件共用同一套时间序列与目录接口，因此控制台图表、回放和导出都能直接使用。

## 注册字段结构

遥测结构必须在构造 `FlyRulerClient` 的时候通过 `telemetry_schemas` 参数注册，连接建立之后再提交遥测不会追加到已有流。

```python
from fly_ruler_proto_python import (
    FlyRulerClient,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
)

engine = TelemetryStreamSchema(
    stream_id="engine",
    fields=[
        TelemetryField(
            "n1",
            label="N1",
            group="engine",
            unit="%",
            value_type=TelemetryValueType.F64,
        ),
        TelemetryField(
            "egt",
            label="EGT",
            group="engine",
            unit="°C",
            value_type=TelemetryValueType.F64,
        ),
        TelemetryField(
            "fuel_kg",
            label="Fuel",
            group="engine",
            unit="kg",
            value_type=TelemetryValueType.F64,
        ),
        TelemetryField(
            "mode", label="Mode", group="engine", value_type=TelemetryValueType.I64
        ),
        TelemetryField(
            "gear_down",
            label="Gear down",
            group="engine",
            value_type=TelemetryValueType.Bool,
        ),
    ],
    name="Engine",
    nominal_rate_hz=10.0,
)

with FlyRulerClient("127.0.0.1:18002", "F-16", telemetry_schemas=(engine,)) as aircraft:
    aircraft.update_state(...)
```

`TelemetryStreamSchema` 的构造参数是 `stream_id`、`fields`、`name` 与 `nominal_rate_hz`，声明在 `bindings/python/src/fly_ruler_proto_python/_core.pyi:286-292`，其中 `fields` 的类型是 `list[TelemetryField]`（`bindings/python/src/fly_ruler_proto_python/_core.pyi:284`）。

`TelemetryField` 的构造参数是 `field_id`、`label`、`group`、`unit`、`description` 与 `value_type`（`bindings/python/src/fly_ruler_proto_python/_core.pyi:251-259`）；`group` 决定这个字段出现在控制台字段目录的哪一组。

`TelemetryValueType` 提供 `F64`、`I64` 与 `Bool` 三个取值（`bindings/python/src/fly_ruler_proto_python/_core.pyi:228-230`），字段构造时默认是 `F64`。

`FlyRulerClient.__init__` 的完整签名是 `(address, aircraft_name, initial_state=None, toml_config="", heartbeat_interval_secs=1.0, telemetry_schemas=(), spawn_timestamp=None)`（`bindings/python/src/fly_ruler_proto_python/client.py:99-108`）。

`telemetry_schemas` 接受 `Sequence[TelemetryStreamSchema]`，在构造时被转成列表交给底层客户端（`bindings/python/src/fly_ruler_proto_python/client.py:134`）。

## 发布数据帧

用 `publish_telemetry(stream_id, values, timestamp=None)` 发送一帧，`values` 的顺序和个数必须与注册 schema 的字段顺序一致，类型按字段声明的 `value_type` 逐项转换。

底层实现见 `bindings/python/src/client.rs:206-248`：未知流抛 `PyKeyError("unknown telemetry stream: {stream_id}")`（`bindings/python/src/client.rs:214-216`），值个数不符抛 `PyValueError("telemetry stream {stream_id} expects {} values, got {value_count}")`（`bindings/python/src/client.rs:218-222`），通过后逐项做类型转换（`bindings/python/src/client.rs:225-227`）。

类型规则是 `F64` 接受整数与浮点、`I64` 拒绝布尔值、`Bool` 只接受布尔值，转换细节在 `bindings/python/src/client.rs:303-338`。

`timestamp` 留空时由客户端填当前时间；每条遥测帧带独立序列号，发送成功后自增（`bindings/python/src/client.rs:229-238`）。

同一时刻只应有一个写者使用同一个客户端实例，跨线程并发发送会打乱序列号与时间戳的对应关系。

## 在控制台里看图

遥测字段会出现在控制台的字段目录里，与飞机状态字段并列。目录由 `/api/v1/series/catalog` 返回（`core/src/management/routes.rs:33-57`），遥测选择器的形式是 `{"kind": "telemetry", "stream_id": "...", "field_id": "..."}`（`web/src/types.ts:105`）。

字段按 `group` 分组显示（`web/src/components/DataSidebar.vue:29-32`），点击某个字段就把对应曲线加入当前图表（`web/src/components/DataSidebar.vue:103`）。

选择与绘制的流程和状态字段完全一致，工具栏、时间轴与工作区的操作方式见[控制台](03-console.md)。

## 完整示例

`bindings/python/examples/03_events_and_telemetry.py` 是可运行的完整示例：它注册一个 `engine` 流，字段为 `n1`、`egt`、`fuel_kg`、`mode`、`gear_down`，覆盖 `F64`、`I64` 与 `Bool` 三种类型，然后在循环里用 `client.publish_telemetry(STREAM_ID, values, timestamp=time.time())` 持续发送。

同一个脚本还发自定义事件 `demo.started` 与周期性的 `flyruler.control.gear_down`、`flyruler.control.gear_up`，用来观察事件与遥测在时间轴上的共存关系。

流声明不合法时构造客户端会抛 `ValueError`，脚本捕获后打印原因并以退出码 1 结束；参数用 `--help` 查看，运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [飞控指令与事件](04-control.md)
- [控制台](03-console.md)
- [会话、回放与导出](06-sessions.md)
