#!/usr/bin/env python3
"""喂给 MSFS 桥接的地理航迹。

对应使用手册 `docs/guide/04-control.md`，其中的「五分钟跑通」直接运行这条命令。

用法：

    uv run python examples/02_control_msfs.py --duration 30
    uv run python examples/02_control_msfs.py --latitude 31.1434 --engine-count 2

脚本围绕一个经纬度中心点做小半径圆周飞行，除位置与姿态外还上报
``DerivedState``（经纬高、真空速、航向）、控制面偏度与发动机油门，这些量正是
MSFS 桥接用来驱动飞机视觉状态的部分。桥接本身只在 Windows/Proton 上运行，本机
没有服务端时脚本打印中文原因并以退出码 1 结束。
"""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import (
    Attitude,
    ControlSurfaceState,
    DerivedState,
    FlyRulerClient,
    PropulsorKind,
    PropulsorState,
    create_aircraft_state,
)

EARTH_RADIUS_M = 6_378_137.0
GEAR_DOWN_EVENT = "flyruler.control.gear_down"
GEAR_UP_EVENT = "flyruler.control.gear_up"


@dataclass(frozen=True)
class ApproachConfig:
    """地理圆周的参数。"""

    latitude_deg: float
    longitude_deg: float
    altitude_m: float
    radius_m: float
    speed_mps: float
    engine_count: int


def build_state(elapsed_s: float, config: ApproachConfig):
    """按经过时间构造带派生量的状态快照。"""
    omega = config.speed_mps / max(config.radius_m, 1e-6)
    phase = omega * elapsed_s

    north_m = config.radius_m * math.cos(phase)
    east_m = config.radius_m * math.sin(phase)
    latitude = config.latitude_deg + math.degrees(north_m / EARTH_RADIUS_M)
    longitude = config.longitude_deg + math.degrees(
        east_m
        / (EARTH_RADIUS_M * max(math.cos(math.radians(config.latitude_deg)), 1e-6))
    )

    velocity_north = -config.radius_m * omega * math.sin(phase)
    velocity_east = config.radius_m * omega * math.cos(phase)
    yaw = math.atan2(velocity_east, velocity_north)
    half = yaw * 0.5

    # 用相位驱动控制面，让桥接侧看到连续变化的操纵输入。
    aileron = 0.12 * math.sin(phase)
    elevator = 0.05 * math.sin(phase * 0.5)
    rudder = 0.08 * math.cos(phase)
    throttle = 0.55 + 0.15 * math.sin(phase * 0.25)

    return create_aircraft_state(
        position=(north_m, east_m, -config.altitude_m),
        # 桥接按机体系（x 前、y 右、z 下）换算模拟器速度。
        velocity=(config.speed_mps, 0.0, 0.0),
        attitude=Attitude.from_quaternion((math.cos(half), 0.0, 0.0, math.sin(half))),
        angular_velocity=(0.0, 0.0, omega),
        derived=DerivedState(
            lat=latitude,
            lon=longitude,
            altitude=config.altitude_m,
            tas=config.speed_mps,
            eas=config.speed_mps,
            chi=yaw,
        ),
        control_surfaces=ControlSurfaceState(
            aileron_left_rad=aileron,
            aileron_right_rad=-aileron,
            elevator_rad=elevator,
            rudder_rad=rudder,
            flaps_left_ratio=0.0,
            flaps_right_ratio=0.0,
            spoilers_ratio=0.0,
        ),
        propulsors=[
            PropulsorState(
                f"engine.{index}",
                kind=PropulsorKind.JET,
                throttle_ratio=throttle,
                index=index,
            )
            for index in range(1, config.engine_count + 1)
        ],
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="喂给 MSFS 桥接的地理航迹")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="MSFSDemo", help="飞机显示名")
    parser.add_argument(
        "--latitude", type=float, default=31.1434, help="圆心纬度 [deg]"
    )
    parser.add_argument(
        "--longitude", type=float, default=121.8052, help="圆心经度 [deg]"
    )
    parser.add_argument("--altitude", type=float, default=1200.0, help="高度 [m]")
    parser.add_argument("--radius", type=float, default=500.0, help="圆周半径 [m]")
    parser.add_argument("--speed", type=float, default=70.0, help="地速 [m/s]")
    parser.add_argument("--hz", type=float, default=60.0, help="上报频率 [Hz]")
    parser.add_argument(
        "--duration", type=float, default=0.0, help="运行时长 [s]，0 表示一直运行"
    )
    parser.add_argument("--engine-count", type=int, default=2, help="发动机数量")
    parser.add_argument(
        "--gear-cycle", type=float, default=8.0, help="起落架事件间隔 [s]，0 表示不发"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.hz <= 0:
        raise SystemExit("--hz 必须大于 0")
    if args.duration < 0:
        raise SystemExit("--duration 不能为负")
    if args.engine_count <= 0:
        raise SystemExit("--engine-count 必须大于 0")
    if args.radius <= 0:
        raise SystemExit("--radius 必须大于 0")

    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    config = ApproachConfig(
        latitude_deg=args.latitude,
        longitude_deg=args.longitude,
        altitude_m=args.altitude,
        radius_m=args.radius,
        speed_mps=args.speed,
        engine_count=args.engine_count,
    )

    try:
        client = FlyRulerClient(
            args.address,
            args.aircraft,
            initial_state=build_state(0.0, config),
            toml_config="[aircraft]\nmodel='msfs_demo'",
        )
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端或 MSFS 桥接已启动（just dev server / just msfs run）。")
        return 1

    with client:
        print(f"aircraft_uuid={client.aircraft_uuid}，{args.hz:g} Hz")
        period = 1.0 / args.hz
        start = time.monotonic()
        next_tick = start
        next_gear = args.gear_cycle
        gear_down = True
        sent = 0

        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break

            client.update_state(build_state(elapsed, config), timestamp=time.time())
            sent += 1

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

    print(f"共上报 {sent} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
