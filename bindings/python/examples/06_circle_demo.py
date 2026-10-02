#!/usr/bin/env python3
"""连续的圆周飞行演示。

用法：

    uv run python examples/06_circle_demo.py --duration 30
    uv run python examples/06_circle_demo.py --radius 600 --speed 80 --event-every 5

这是最接近真实使用方式的示例：一架飞机在水平面内做匀速圆周运动，姿态只含
偏航、角速度恒定，周期性发送自定义事件标记飞行阶段。它适合配合控制台一起
观察时间序列曲线与回放时间轴。
"""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import Attitude, FlyRulerClient, create_aircraft_state


@dataclass(frozen=True)
class MotionConfig:
    """圆周运动的参数。"""

    radius_m: float
    speed_mps: float
    altitude_m: float


def build_state(elapsed_s: float, config: MotionConfig):
    """按经过时间构造圆周飞行状态。"""
    omega = config.speed_mps / max(config.radius_m, 1e-6)
    theta = omega * elapsed_s

    north_m = config.radius_m * math.cos(theta)
    east_m = config.radius_m * math.sin(theta)
    velocity_north = -config.radius_m * omega * math.sin(theta)
    velocity_east = config.radius_m * omega * math.cos(theta)

    yaw = math.atan2(velocity_east, velocity_north)
    half = yaw * 0.5
    return create_aircraft_state(
        position=(north_m, east_m, -config.altitude_m),
        # 机体系速度：协调转弯时侧滑为零，只有前向分量。
        velocity=(config.speed_mps, 0.0, 0.0),
        attitude=Attitude.from_quaternion((math.cos(half), 0.0, 0.0, math.sin(half))),
        angular_velocity=(0.0, 0.0, omega),
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="连续的圆周飞行演示")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="CircleDemo", help="飞机显示名")
    parser.add_argument("--hz", type=float, default=30.0, help="上报频率 [Hz]")
    parser.add_argument(
        "--duration", type=float, default=0.0, help="运行时长 [s]，0 表示一直运行"
    )
    parser.add_argument("--radius", type=float, default=300.0, help="圆周半径 [m]")
    parser.add_argument("--speed", type=float, default=60.0, help="地速 [m/s]")
    parser.add_argument("--altitude", type=float, default=1200.0, help="高度 [m]")
    parser.add_argument(
        "--event-every", type=float, default=5.0, help="事件间隔 [s]，0 表示不发"
    )
    parser.add_argument(
        "--toml-config",
        default="[aircraft]\nmodel='circle_demo'",
        help="随 spawn 一起发送的 TOML 配置文本",
    )
    parser.add_argument("--heartbeat", type=float, default=1.0, help="心跳间隔 [s]")
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

    config = MotionConfig(
        radius_m=args.radius,
        speed_mps=args.speed,
        altitude_m=args.altitude,
    )

    print(f"连接 {args.address}，飞机 {args.aircraft}")
    try:
        client = FlyRulerClient(
            args.address,
            args.aircraft,
            initial_state=build_state(0.0, config),
            toml_config=args.toml_config,
            heartbeat_interval_secs=args.heartbeat,
        )
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动（just dev server）。")
        return 1

    with client:
        print(f"aircraft_uuid={client.aircraft_uuid}")
        period = 1.0 / args.hz
        start = time.monotonic()
        next_tick = start
        next_event = args.event_every
        sent = 0

        try:
            while running:
                elapsed = time.monotonic() - start
                if args.duration > 0 and elapsed >= args.duration:
                    break

                client.update_state(build_state(elapsed, config), timestamp=time.time())
                sent += 1

                if args.event_every > 0 and elapsed >= next_event:
                    client.create_event("demo.tick", timestamp=time.time())
                    next_event += args.event_every

                next_tick += period
                delay = next_tick - time.monotonic()
                if delay > 0:
                    time.sleep(delay)
                else:
                    next_tick = time.monotonic()
        finally:
            client.create_event("demo.finished", timestamp=time.time())

    print(f"共上报 {sent} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
