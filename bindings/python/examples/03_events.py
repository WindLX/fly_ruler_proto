#!/usr/bin/env python3
"""在状态流里插入命名事件。

用法：

    uv run python examples/03_events.py
    uv run python examples/03_events.py --duration 8 --gear-cycle 3

事件和时间序列是两条独立的记录：状态走 ``update_state``，事件走
``create_event``，两者都带自己的时间戳。脚本先发一个自定义事件，然后周期性
交替发送起落架收起与放下事件，用来观察回放时间轴上的事件标记。
"""

from __future__ import annotations

import argparse
import signal
import time

from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state

GEAR_DOWN_EVENT = "flyruler.control.gear_down"
GEAR_UP_EVENT = "flyruler.control.gear_up"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="在状态流里插入命名事件")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="EventDemo", help="飞机显示名")
    parser.add_argument("--hz", type=float, default=10.0, help="状态上报频率 [Hz]")
    parser.add_argument("--duration", type=float, default=6.0, help="运行时长 [s]")
    parser.add_argument(
        "--gear-cycle", type=float, default=2.0, help="起落架事件间隔 [s]，0 表示不发"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.hz <= 0:
        raise SystemExit("--hz 必须大于 0")

    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    try:
        client = FlyRulerClient(
            args.address,
            args.aircraft,
            initial_state=create_aircraft_state(),
        )
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动（just dev server）。")
        return 1

    period = 1.0 / args.hz
    start = time.monotonic()
    next_tick = start
    next_gear = args.gear_cycle
    gear_down = True

    try:
        # 自定义事件名不做保留字校验，命名空间由服务端使用方自行约定。
        client.create_event("demo.started", timestamp=time.time())
        print("已发送事件 demo.started")

        while running:
            elapsed = time.monotonic() - start
            if elapsed >= args.duration:
                break

            # 状态帧本身不携带事件，事件要单独发。
            client.update_state(create_aircraft_state(), timestamp=time.time())

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

    print("连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
