#!/usr/bin/env python3
"""为 MSFS 桥接构造一支 AI 飞机编队。

用法：

    uv run python examples/08_msfs_ai_fleet.py --fleet-size 4 --duration 30
    uv run python examples/08_msfs_ai_fleet.py --latitude 31.1434 --spacing 0.004

每架飞机是一个独立客户端，围绕各自的经纬度中心做小半径圆周飞行，编队成员
沿东西向等距排开、相位错开。桥接开启 AI 飞机开关后会用这些飞机创建模拟器里
的 AI 机队，因此这里的状态除了位置、姿态、派生量之外还包含控制面与发动机。
与 07 一样，连接失败时以退出码 0 结束。
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


@dataclass(frozen=True)
class LeaderConfig:
    """编队基准点与队形参数。"""

    latitude_deg: float
    longitude_deg: float
    altitude_m: float
    radius_m: float
    speed_mps: float
    spacing_deg: float
    engine_count: int


def build_state(elapsed_s: float, config: LeaderConfig, index: int, count: int):
    """构造编队中第 index 架飞机的状态快照。"""
    omega = config.speed_mps / max(config.radius_m, 1e-6)
    # 相邻成员相位错开半个队形间隔，避免所有飞机同时转向。
    phase = omega * elapsed_s + 2.0 * math.pi * index / max(count, 1)

    latitude_center = (
        config.latitude_deg + (index - (count - 1) / 2.0) * config.spacing_deg
    )
    north_m = config.radius_m * math.cos(phase)
    east_m = config.radius_m * math.sin(phase)
    latitude = latitude_center + math.degrees(north_m / EARTH_RADIUS_M)
    longitude = config.longitude_deg + math.degrees(
        east_m / (EARTH_RADIUS_M * max(math.cos(math.radians(latitude_center)), 1e-6))
    )

    velocity_north = -config.radius_m * omega * math.sin(phase)
    velocity_east = config.radius_m * omega * math.cos(phase)
    yaw = math.atan2(velocity_east, velocity_north)
    half = yaw * 0.5

    bank = 0.25 * math.sin(phase)
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
            aileron_left_rad=-bank,
            aileron_right_rad=bank,
            elevator_rad=0.02 * math.sin(phase * 0.5),
            rudder_rad=0.0,
            flaps_left_ratio=0.0,
            flaps_right_ratio=0.0,
            spoilers_ratio=0.0,
        ),
        propulsors=[
            PropulsorState(
                f"engine.{position}",
                kind=PropulsorKind.JET,
                throttle_ratio=0.5 + 0.1 * math.sin(phase * 0.25),
                index=position,
            )
            for position in range(1, config.engine_count + 1)
        ],
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="为 MSFS 桥接构造一支 AI 飞机编队")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--prefix", default="MSFS-AI", help="飞机名前缀")
    parser.add_argument("--fleet-size", type=int, default=3, help="编队飞机数量")
    parser.add_argument(
        "--latitude", type=float, default=31.1434, help="基准纬度 [deg]"
    )
    parser.add_argument(
        "--longitude", type=float, default=121.8052, help="基准经度 [deg]"
    )
    parser.add_argument("--altitude", type=float, default=900.0, help="高度 [m]")
    parser.add_argument("--radius", type=float, default=400.0, help="圆周半径 [m]")
    parser.add_argument("--speed", type=float, default=65.0, help="地速 [m/s]")
    parser.add_argument(
        "--spacing", type=float, default=0.003, help="相邻飞机的纬度间隔 [deg]"
    )
    parser.add_argument("--hz", type=float, default=30.0, help="上报频率 [Hz]")
    parser.add_argument(
        "--duration", type=float, default=0.0, help="运行时长 [s]，0 表示一直运行"
    )
    parser.add_argument(
        "--engine-count", type=int, default=2, help="每架飞机的发动机数量"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.fleet_size <= 0:
        raise SystemExit("--fleet-size 必须大于 0")
    if args.hz <= 0:
        raise SystemExit("--hz 必须大于 0")

    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    config = LeaderConfig(
        latitude_deg=args.latitude,
        longitude_deg=args.longitude,
        altitude_m=args.altitude,
        radius_m=args.radius,
        speed_mps=args.speed,
        spacing_deg=args.spacing,
        engine_count=args.engine_count,
    )

    names = [f"{args.prefix}-{index + 1}" for index in range(args.fleet_size)]
    clients: list[FlyRulerClient] = []

    try:
        for index, name in enumerate(names):
            clients.append(
                FlyRulerClient(
                    args.address,
                    name,
                    initial_state=build_state(0.0, config, index, args.fleet_size),
                    toml_config="[aircraft]\nmodel='msfs_ai'",
                )
            )
            print(f"{name} 已连接 aircraft_uuid={clients[-1].aircraft_uuid}")
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("MSFS 桥接只在 Windows/Proton 上运行，本机没有服务端时示例直接结束。")
        for client in clients:
            client.close()
        return 0

    period = 1.0 / args.hz
    start = time.monotonic()
    next_tick = start
    frames = 0

    try:
        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break

            timestamp = time.time()
            for index, client in enumerate(clients):
                client.update_state(
                    build_state(elapsed, config, index, args.fleet_size),
                    timestamp=timestamp,
                )
            frames += 1

            next_tick += period
            delay = next_tick - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            else:
                next_tick = time.monotonic()
    finally:
        for client in clients:
            client.close()

    print(f"{len(clients)} 架 AI 飞机各上报 {frames} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
