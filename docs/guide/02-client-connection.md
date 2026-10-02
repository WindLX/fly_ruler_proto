# 客户端连接与状态更新

`FlyRulerClient` 的每个实例对应服务端的一条 UDP 会话和一架飞机：构造时握手并发送 spawn，之后可以持续推送状态、事件与遥测，退出时关闭会话。

状态更新是单向的 best-effort 数据报，服务端只按时间戳把样本写进时间序列，不回执每次写入。

## 建立会话

```python
FlyRulerClient(
    address,
    aircraft_name,
    initial_state=None,
    toml_config="",
    heartbeat_interval_secs=1.0,
    telemetry_schemas=(),
    spawn_timestamp=None,
)
```

完整签名见 `bindings/python/src/fly_ruler_proto_python/client.py:99-108`。`address` 是服务端 UDP 监听地址，例如 `"127.0.0.1:18002"`。

构造过程是同步的：绑定本地 UDP 端口、发送握手、等待 ACK，超时上限 1 秒（`core/src/transport/client.rs:18`，`core/src/transport/client.rs:276-287`）。服务端校验握手里的协议版本与角色，版本不符回 `ProtocolVersionMismatch`，角色未指定回 `InvalidState`（`core/src/transport/server.rs:445-467`）。

握手成功后客户端分配两个 UUID：`client_uuid` 标识会话，`aircraft_uuid` 标识飞机（`core/src/transport/client.rs:374-375`），两者都以小写十六进制字符串暴露（`bindings/python/src/fly_ruler_proto_python/client.py:139-155`）。服务端用 `aircraft_uuid` 作为时间序列的 id（`core/src/store.rs:30-31`）。

```python
with FlyRulerClient("127.0.0.1:18002", "F-16") as aircraft:
    print(aircraft.client_uuid)
    print(aircraft.aircraft_uuid)
```

`spawn_timestamp` 决定 spawn 事件落在源时间轴上的哪个时刻，省略时用当前 Unix 秒（`core/src/transport/client.rs:69-93`，`core/src/transport/client.rs:36-46`）；产出方改用从 0 开始的仿真时钟时，这个起点要显式给出，非有限值在构造时抛 `ValueError`（`bindings/python/src/fly_ruler_proto_python/client.py:123`）。

`heartbeat_interval_secs` 必须是有限正数，否则构造抛 `ValueError`（`bindings/python/src/fly_ruler_proto_python/client.py:124-127`）；心跳任务实际使用的时间间隔不小于 0.1 秒（`core/src/transport/client.rs:497-528`）。心跳同时携带 `client_uuid`，服务端据此把后续数据报归属到会话（`core/src/transport/server.rs:325-342`）；超过 `heartbeat_timeout_secs` 没有收到任何包，会话会被清理（`core/src/transport/server.rs:58-65`）。

## create_aircraft_state 参数

| 参数 | 含义 |
| --- | --- |
| `position` | NED 位置 `(x, y, z)`，米（`bindings/python/src/fly_ruler_proto_python/client.py:48`） |
| `velocity` | 速度 `(vx, vy, vz)`，米/秒（`bindings/python/src/fly_ruler_proto_python/client.py:49`） |
| `attitude` | body-FRD 到局部 NED 的姿态，省略时为 `Attitude.identity()`（`bindings/python/src/protocol.rs:51-57`） |
| `angular_velocity` | 机体角速度 `(p, q, r)`，弧度/秒（`bindings/python/src/fly_ruler_proto_python/client.py:51`） |
| `derived` | `DerivedState` 派生量：纬度、经度、高度、迎角、侧滑角、TAS/EAS、航迹角，以及可选的 IAS/CAS/马赫数等（`bindings/python/src/protocol.rs:130-165`） |
| `control_surfaces` | `ControlSurfaceState`，7 个可选的操纵面弧度或比值（`bindings/python/src/protocol.rs:242-300`） |
| `linear_acceleration_body` | 机体坐标系加速度，米/秒²（`bindings/python/src/fly_ruler_proto_python/client.py:54`） |
| `propulsors` | `PropulsorState` 列表，逐项带 `propulsor_id` 与 `kind`（`bindings/python/src/protocol.rs:303-314`） |

向量参数接受长度为 3 的序列；姿态可以用 `Attitude.from_quaternion`、`Attitude.from_rotation_matrix` 或 `Attitude.from_euler` 构造，欧拉角按 Z-Y-X 顺序解读（`bindings/python/src/protocol.rs:68-104`）。省略的参数会填成零向量、单位姿态与空列表（`bindings/python/src/protocol.rs:530-562`）。

```python
from fly_ruler_proto_python import (
    Attitude,
    ControlSurfaceState,
    DerivedState,
    FlyRulerClient,
    PropulsorKind,
    PropulsorState,
    create_aircraft_state,
)

state = create_aircraft_state(
    position=(152.0, -40.0, -1200.0),
    velocity=(180.0, 2.0, 1.5),
    attitude=Attitude.from_euler((0.02, -0.01, 1.2)),
    angular_velocity=(0.0, 0.02, 0.05),
    derived=DerivedState(
        lat=39.9,
        lon=116.4,
        altitude=1200.0,
        alpha=0.03,
        beta=0.001,
        tas=180.0,
        eas=170.0,
        gamma=0.0,
        chi=1.2,
        mach=0.53,
    ),
    control_surfaces=ControlSurfaceState(elevator_rad=-0.01, aileron_left_rad=0.01),
    linear_acceleration_body=(0.2, 0.0, -9.8),
    propulsors=[PropulsorState("engine-1", kind=PropulsorKind.JET, throttle_ratio=0.7)],
)

with FlyRulerClient("127.0.0.1:18002", "F-16", initial_state=state) as aircraft:
    aircraft.update_state(state)
```

把 `initial_state` 传给构造函数，服务端会在这个 spawn 的时间戳上额外写入一条状态样本（`core/src/store.rs:371-376`）；不传时时间序列里只有之后 `update_state` 的样本。

> `velocity` 的坐标系在源码注释里有两种说法：Python docstring 写 NED 速度（`bindings/python/src/fly_ruler_proto_python/client.py:49`），wire schema 则把 `velocity` 与 `linear_acceleration_body` 都定义为 body-FRD，即 x 前、y 右、z 下（`core/proto/fly_ruler.proto:110-117`）。以 wire schema 的注释为准。

## update_state 的时间戳语义

```python
aircraft.update_state(state)                  # 客户端填当前 Unix 秒
aircraft.update_state(state, timestamp=12.5)  # 显式指定源时间轴上的秒
```

`timestamp` 是产出方定义的源时间轴上的有限秒数：可以用 Unix 时间，也可以用从 0 开始的仿真时钟，但同一会话必须保持同一基准（`core/proto/fly_ruler.proto:261-264`）。不传时由客户端填 `SystemTime::now()` 相对 Unix epoch 的秒数（`core/src/transport/client.rs:36-46`）。

`timestamp` 不是有限值会在发送前抛 `ValueError`（`bindings/python/src/fly_ruler_proto_python/client.py:171`）。

服务端始终按时间戳保序存储：比最后一个样本新就追加，否则用二分查找插入（`core/src/store.rs:200-229`），因此迟到的状态也会落到正确的历史位置；时间戳非有限的状态会被服务端丢弃并记一条 warn 日志（`core/src/store.rs:201-204`）。

时间戳同时决定回放游标解析与图表横轴：按时刻取状态时服务端取不晚于该时刻的最近样本（`core/src/store.rs:415-433`）。

## 事件、遥测与心跳

`create_event(event_name, timestamp=None)` 写一条自定义事件（`bindings/python/src/fly_ruler_proto_python/client.py:174-189`），事件名会出现在时间线事件查询结果里。

`publish_telemetry(stream_id, values, timestamp=None)` 按 spawn 时声明的流发送样本，细节见 [遥测与时间序列](/guide/components/proto/03-telemetry)。

心跳在后台按间隔自动发送，服务端对心跳回 ACK（`core/src/transport/server.rs:468-470`），客户端本身不读取这些 ACK，心跳的唯一作用是让会话保持存活。

## 结束会话

```python
aircraft.despawn(reason="landed")
aircraft.close()
```

`despawn(reason=None, timestamp=None)` 立即结束飞机生命周期，重复调用直接返回而不重复发送（`core/src/transport/client.rs:597-610`）。

`close()` 是幂等的（`bindings/python/src/fly_ruler_proto_python/client.py:225-229`）：若此前的 despawn 没有显式发送过，`close()` 会补发一条 reason 为 `client_close` 的 despawn（`core/src/transport/client.rs:626-634`），然后停止心跳、关闭发送任务并按顺序等待后台任务退出（`core/src/transport/client.rs:636-657`）。

上下文管理器在退出时调用 `close()` 并屏蔽退出路径上的异常（`bindings/python/src/fly_ruler_proto_python/client.py:231-252`），`__del__` 只作为兜底（`bindings/python/src/fly_ruler_proto_python/client.py:254-259`）。

`despawn()` 与对象销毁的区别在于语义和时机：despawn 会在时间序列里落下一条 `despawn` 事件，回放时该时刻之后的 `is_spawned_at` 为 false（`core/src/store.rs:436-461`）；而对象销毁只保证 `close()` 被调用一次，发生时机由垃圾回收决定，不代表飞机在时间线上结束。

关闭之后继续调用会抛 `ConnectionError: client is closed`（`bindings/python/src/client.rs:340-348`）。

## best-effort 语义

只有握手与心跳有 ACK：`update_state`、`create_event`、`publish_telemetry` 把消息放进发送队列后立即返回，不等待响应（`core/src/transport/client.rs:555-594`）；服务端对成功处理的飞机事件也不回 ACK（`core/src/transport/server.rs:526-530`）。

这意味着状态与遥测在 UDP 上不重传，丢包就是样本缺失；同时客户端看不到服务端拒绝事件的原因，服务端只在自己的日志里记录非 producer 会话的拒绝信息（`core/src/transport/server.rs:501-511`）。

配套的可运行示例是 `bindings/python/examples/02_state_update.py` 与 `bindings/python/examples/03_events.py`；示例清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [安装与快速开始](/guide/components/proto/01-install)
- [遥测与时间序列](/guide/components/proto/03-telemetry)
- [回放与游标](/guide/components/proto/04-playback)
- [UDP 会话与错误处理](/dev/components/proto/03-udp-session)
- [协议 API 参考](/api/proto/)
