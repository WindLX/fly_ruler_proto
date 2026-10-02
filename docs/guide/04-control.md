# 用 Python client 控制飞行

Python client 只做一件事：把飞机状态推给服务端。服务端可能是独立进程，也可能是 MSFS 桥内嵌的那一个；对客户端而言没有区别，都是往 `127.0.0.1:18002` 发 UDP 报文。

## 最小程序

把下面这段存成 `demo.py` 就能跑，前提是服务端已在本地运行、客户端库已按 `01-install.md` 装好：

```python
import math
import time

from fly_ruler_proto_python import (
    Attitude, ControlSurfaceState, DerivedState, FlyRulerClient,
    PropulsorKind, PropulsorState, create_aircraft_state,
)


def frame(elapsed_s):
    phase = 0.14 * elapsed_s
    half = phase * 0.5
    return create_aircraft_state(
        position=(500.0 * math.cos(phase), 500.0 * math.sin(phase), -1200.0),
        velocity=(70.0, 0.0, 0.0),
        attitude=Attitude.from_quaternion((math.cos(half), 0.0, 0.0, math.sin(half))),
        angular_velocity=(0.0, 0.0, 0.14),
        derived=DerivedState(lat=31.1434, lon=121.8052, altitude=1200.0, tas=70.0),
        control_surfaces=ControlSurfaceState(
            aileron_left_rad=0.12, aileron_right_rad=-0.12,
            elevator_rad=0.0, rudder_rad=0.0,
            flaps_left_ratio=0.0, flaps_right_ratio=0.0, spoilers_ratio=0.0,
        ),
        propulsors=[PropulsorState("engine.1", kind=PropulsorKind.JET, throttle_ratio=0.6)],
    )


with FlyRulerClient("127.0.0.1:18002", "MSFSDemo", initial_state=frame(0.0)) as client:
    start = time.monotonic()
    while True:
        client.update_state(frame(time.monotonic() - start), timestamp=time.time())
        time.sleep(1.0 / 60.0)
```

这里的构造方式与 `bindings/python/examples/02_control_msfs.py:52-108` 一致，只是省掉了事件和命令行参数。要更完整地看圆周航迹、起落架事件与信号处理，直接读那个示例。

## 连接与生命周期

`FlyRulerClient(address, aircraft_name, initial_state=None, toml_config="", heartbeat_interval_secs=1.0, telemetry_schemas=(), spawn_timestamp=None)` 在构造时就完成连接、握手并生成一架飞机（`bindings/python/src/fly_ruler_proto_python/client.py:99-108`）。构造函数内部把 `initial_state` 为 `None` 的情况替换成默认状态副本，失败会直接抛异常，不会留下半初始化的实例（`bindings/python/src/fly_ruler_proto_python/client.py:128-137`）。

握手报文带上协议版本、`client_uuid` 和 `role: Producer`（`core/src/transport/client.rs:48-57`，其中 `:53` 是角色），客户端最多等 1 秒 ACK，超时即失败（`core/src/transport/client.rs:18`、`:276-287`）。会话建立后按 `heartbeat_interval_secs` 周期发心跳，实际间隔下限是 0.1 秒（`core/src/transport/client.rs:59-67`、`:498`）。

`close()` 幂等，重复调用没有额外效果；用 `with FlyRulerClient(...) as client:` 时退出代码块会自动关闭（`bindings/python/src/fly_ruler_proto_python/client.py:225-252`）。`client_uuid` 与 `aircraft_uuid` 在连接成功后可用，前者标识这条连接，后者标识这架飞机（`bindings/python/src/fly_ruler_proto_python/client.py:139-155`）。

## 构造一架飞机

`create_aircraft_state` 的完整签名是 `create_aircraft_state(position, velocity, attitude, angular_velocity, derived, control_surfaces, linear_acceleration_body, propulsors)`，每个参数都有默认值（`bindings/python/src/fly_ruler_proto_python/client.py:35-73`）：

- `position` 是 NED 位置三元组 `[m]`；`velocity` 是机体系 BODY-FRD 速度三元组 `[m/s]`，即前、右、下三个方向的分量。
- `attitude` 用 `Attitude.from_quaternion((w, x, y, z))` 构造，也可以用 `Attitude.from_euler`、`Attitude.from_rotation_matrix` 或 `Attitude.identity()`。
- `derived` 传 `DerivedState`，纬度、经度、高度是三个必填项，迎角、侧滑角、真空速、马赫数等可以留默认。
- `control_surfaces` 传 `ControlSurfaceState`，包含左右副翼、升降舵、方向舵、左右襟翼与扰流板，共七个操纵面。
- `propulsors` 是 `PropulsorState` 列表，每台至少要有 `propulsor_id` 和 `kind`，油门、转速、桨距、推力、扭矩按需填。

字段类型与默认值以 `bindings/python/src/fly_ruler_proto_python/_core.pyi` 为准，那是绑定公开面的权威清单。

## 推送状态

`update_state(state, timestamp=None)` 发送一帧状态（`bindings/python/src/fly_ruler_proto_python/client.py:157-172`）。`timestamp` 省略时客户端用本地时钟（相对 Unix epoch 的秒，`core/src/transport/client.rs:36-46`）；显式传 `time.time()` 与省略等价。

控制台时间轴按这个时间排序，所以想得到稳定的回放曲线就自己给单调递增的时间戳。时间戳乱跳会让曲线回折，事件也会落在意外位置。

## 事件与控制面

`create_event(event_name, timestamp=None)` 发一条命名事件（`bindings/python/src/fly_ruler_proto_python/client.py:174-189`）。多机场景里起落架有约定的名字：`flyruler.control.gear_up` 与 `flyruler.control.gear_down`（`core/src/events.rs:4`、`:7`），MSFS 桥识别这两个事件并驱动机模起落架动画（`bindings/msfs/src/lib.rs:308-309`）。

除事件外，每帧的 `ControlSurfaceState` 携带七个操纵面偏度，油门走 `PropulsorState.throttle_ratio`。`examples/03_events_and_telemetry.py` 演示事件与遥测，`examples/02_control_msfs.py` 演示「七个操纵面 + 多台发动机」的完整状态。

## 多架飞机

一个 `FlyRulerClient` 实例对应一架飞机。要同时控制多架，就为每架建一个客户端，可以共用同一个服务端地址，退出时统一 `close()`；完整写法见 `examples/05_multi_aircraft.py`（`bindings/python/examples/05_multi_aircraft.py:102-117`）。

MSFS 侧默认只驱动一架用户机，想让每架 FlyRuler 飞机都映射成一个 AI 机模要打开 `--enable-ai-aircraft`（`bindings/msfs/src/config.rs:36`），映射与上限规则见开发手册的 `docs/dev/04-msfs-binding.md`。

## client 不做的事

client 只能推送：`update_state`、`create_event`、`publish_telemetry`、`despawn` 都是单向发送，没有拉取接口（`bindings/python/src/fly_ruler_proto_python/client.py:157-223`）。要读数据有两个途径：打开控制台看曲线与状态，或者直接调 HTTP 管理接口，例如 `/api/v1/health`、`/api/v1/status`、`/api/v1/aircraft`（`core/src/management/routes.rs:33-57`）。遥测流的注册方式见 `05-telemetry.md`。

## 相关页面

- [在 web console 里管理](03-console.md)
- [遥测数据](05-telemetry.md)
- [排障](07-troubleshooting.md)
