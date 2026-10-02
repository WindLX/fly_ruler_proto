#!/usr/bin/env python3
"""注册遥测流、发布数据帧，并在状态流里插入事件。

对应使用手册 `docs/guide/05-telemetry.md`。

用法：

    uv run python examples/03_events_and_telemetry.py
    uv run python examples/03_events_and_telemetry.py --hz 5 --duration 10 --gear-cycle 3

遥测流采用两段式：创建客户端时用 ``TelemetryStreamSchema`` 注册字段结构，之后用
``publish_telemetry`` 按字段声明顺序发布取值序列，服务端用注册的类型解出数值。
事件走 ``create_event``，与状态、遥测各自独立记录：脚本先发一个自定义事件，再周期
性交替发送起落架收起与放下事件，用来观察回放时间轴上的事件标记。
``--duration 0`` 表示一直运行到中断。

服务端不可达时打印中文原因并以退出码 1 结束；``Ctrl-C`` 会走完 ``finally``
关闭客户端后退出。
"""

from __future__ import annotations

import argparse
import math
import signal
import time

from fly_ruler_proto_python import (
    FlyRulerClient,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
    create_aircraft_state,
)

STREAM_ID = "engine"
GEAR_DOWN_EVENT = "flyruler.control.gear_down"
GEAR_UP_EVENT = "flyruler.control.gear_up"


def build_schema(nominal_rate_hz: float | None) -> TelemetryStreamSchema:
    """声明一条发动机遥测流的字段结构。"""
    return TelemetryStreamSchema(
        STREAM_ID,
        (
            TelemetryField(
                "n1",
                label="N1",
                group="engine",
                unit="%",
                value_type=TelemetryValueType.F64,
            ),
            TelemetryField(
                "egt",
                label="排气温度",
                group="engine",
                unit="C",
                value_type=TelemetryValueType.F64,
            ),
            TelemetryField(
                "fuel_kg",
                label="剩余燃油",
                group="fuel",
                unit="kg",
                value_type=TelemetryValueType.F64,
            ),
            TelemetryField(
                "mode",
                label="工作模式",
                group="engine",
                value_type=TelemetryValueType.I64,
            ),
            TelemetryField(
                "gear_down",
                label="起落架放下",
                group="gear",
                value_type=TelemetryValueType.Bool,
            ),
        ),
        name="发动机遥测",
        nominal_rate_hz=nominal_rate_hz,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="声明遥测流、发布数据帧并发送事件")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="TelemetryDemo", help="飞机显示名")
    parser.add_argument("--hz", type=float, default=10.0, help="发布频率 [Hz]")
    parser.add_argument(
        "--duration", type=float, default=6.0, help="运行时长 [s]，0 表示一直运行"
    )
    parser.add_argument(
        "--nominal-rate", type=float, default=10.0, help="声明频率 [Hz]，0 表示不声明"
    )
    parser.add_argument(
        "--gear-cycle", type=float, default=2.0, help="起落架事件间隔 [s]，0 表示不发"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.hz <= 0:
        raise SystemExit("--hz 必须大于 0")
    if args.duration < 0:
        raise SystemExit("--duration 不能为负")

    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    nominal_rate_hz = args.nominal_rate if args.nominal_rate > 0 else None
    schema = build_schema(nominal_rate_hz)
    try:
        # 流的字段结构在注册后不可变更，重名或空字段 id 会在构造时报 ValueError。
        client = FlyRulerClient(
            args.address,
            args.aircraft,
            initial_state=create_aircraft_state(),
            telemetry_schemas=(schema,),
        )
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动（just dev server）。")
        return 1
    except ValueError as error:
        print(f"遥测流声明不合法：{error}")
        return 1

    period = 1.0 / args.hz
    start = time.monotonic()
    next_tick = start
    next_gear = args.gear_cycle
    gear_down = True
    published = 0

    try:
        # 自定义事件名不做保留字校验，命名空间由服务端使用方自行约定。
        client.create_event("demo.started", timestamp=time.time())
        print("已发送事件 demo.started")

        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break

            # 状态帧本身不携带事件，事件与遥测都要单独发。
            client.update_state(create_aircraft_state(), timestamp=time.time())

            # 取值顺序必须与字段声明顺序一致。
            values = (
                85.0 + 3.0 * math.sin(elapsed),  # n1
                620.0 + 15.0 * math.sin(elapsed * 0.5),  # egt
                2400.0 - 4.0 * elapsed,  # fuel_kg
                2,  # mode
                elapsed > 1.0,  # gear_down
            )
            client.publish_telemetry(STREAM_ID, values, timestamp=time.time())
            published += 1

            if args.gear_cycle > 0 and elapsed >= next_gear:
                gear_down = not gear_down
                event_name = GEAR_DOWN_EVENT if gear_down else GEAR_UP_EVENT
                client.create_event(event_name, timestamp=time.time())
                print(f"{elapsed:5.1f} s 已发送事件 {event_name}")
                next_gear += args.gear_cycle

            next_tick += period
            delay = next_tick - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            else:
                next_tick = time.monotonic()
    finally:
        client.close()

    print(f"流 {STREAM_ID} 共发布 {published} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
