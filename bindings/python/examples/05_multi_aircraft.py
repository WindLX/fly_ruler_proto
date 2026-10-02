#!/usr/bin/env python3
"""同时驱动多架飞机。

对应使用手册 `docs/guide/03-console.md`。

用法：

    uv run python examples/05_multi_aircraft.py
    uv run python examples/05_multi_aircraft.py --count 5 --duration 8 --radius 800

前置条件：本机已装好客户端库，并有一个服务端在 ``--address``（默认
``127.0.0.1:18002``）上运行，``just dev server`` 是独立服务端的启动命令。多机示例
只推位置、姿态与角速度，喂模拟器需要桥的 AI 飞机开关，仅看曲线时独立服务端就够。

每个 ``FlyRulerClient`` 只代表一架飞机，多机就是多个客户端。脚本把飞机均匀放在
同一个圆周上，相位各不相同，然后在同一个循环里逐架上报状态；控制台会把这些飞机
并排列出，可以逐架选择字段对比曲线。``--duration 0`` 表示一直运行到中断；退出时
按创建顺序逐架关闭，避免留下悬挂的心跳线程。

跑通后每架飞机各打印一行 ``已连接 aircraft_uuid=...``，结束时打印架数与总帧数。
控制台左栏按 ``--prefix`` 编号并排列出这些飞机，选择同一段 ``--duration`` 后它们
的位置曲线是同一条圆上相位错开的几段，每架的事件计数都为 0。

服务端不可达时打印中文原因并以退出码 1 结束；``Ctrl-C`` 会走完 ``finally``
关闭全部客户端后退出。
"""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import Attitude, FlyRulerClient, create_aircraft_state


@dataclass(frozen=True)
class FleetMember:
    """一架飞机及其在编队中的相位。"""

    name: str
    phase_rad: float


def build_state(
    elapsed_s: float, member: FleetMember, radius_m: float, altitude_m: float
):
    """构造编队成员的状态快照。"""
    # 角速度固定为 0.15 rad/s，相位由成员编号对应的初始相位叠加时间项得到。
    omega = 0.15
    theta = member.phase_rad + omega * elapsed_s
    # 编队中心取一个公共的 NED 原点，各成员围绕它排布。
    center_north = 2000.0
    center_east = 1500.0

    north_m = center_north + radius_m * math.cos(theta)
    east_m = center_east + radius_m * math.sin(theta)
    velocity_north = -radius_m * omega * math.sin(theta)
    velocity_east = radius_m * omega * math.cos(theta)

    # 航向角取切向速度方向，姿态四元数只含这个偏航分量。
    yaw = math.atan2(velocity_east, velocity_north)
    half = yaw * 0.5
    return create_aircraft_state(
        position=(north_m, east_m, -altitude_m),
        # 机体系速度：协调转弯时侧滑为零，只有前向分量。
        velocity=(radius_m * omega, 0.0, 0.0),
        attitude=Attitude.from_quaternion((math.cos(half), 0.0, 0.0, math.sin(half))),
        angular_velocity=(0.0, 0.0, omega),
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="同时驱动多架飞机")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--prefix", default="Fleet", help="飞机名前缀")
    parser.add_argument("--count", type=int, default=3, help="飞机数量")
    parser.add_argument(
        "--hz", type=float, default=20.0, help="每架飞机的上报频率 [Hz]"
    )
    parser.add_argument(
        "--duration", type=float, default=6.0, help="运行时长 [s]，0 表示一直运行"
    )
    parser.add_argument("--radius", type=float, default=500.0, help="编队半径 [m]")
    parser.add_argument("--altitude", type=float, default=1500.0, help="高度 [m]")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.count <= 0:
        raise SystemExit("--count 必须大于 0")
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

    # 相位按 2π/count 均分，保证整圈上各机分散均匀。
    members = [
        FleetMember(f"{args.prefix}-{index + 1}", 2.0 * math.pi * index / args.count)
        for index in range(args.count)
    ]
    clients: list[FlyRulerClient] = []

    try:
        # 逐架构造客户端即逐架握手与 spawn，失败时把已建好的连接全部关掉。
        for member in members:
            clients.append(
                FlyRulerClient(
                    args.address,
                    member.name,
                    initial_state=build_state(0.0, member, args.radius, args.altitude),
                )
            )
            print(f"{member.name} 已连接 aircraft_uuid={clients[-1].aircraft_uuid}")
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动（just dev server）。")
        for client in clients:
            client.close()
        return 1

    # 一个节拍遍历全部飞机，同一帧内共用一个时间戳。
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
            for member, client in zip(members, clients):
                # 每架飞机只发自己那份状态，共享时间戳保证多机曲线对齐。
                client.update_state(
                    build_state(elapsed, member, args.radius, args.altitude),
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
        # 逐架关闭会停止各自的心跳与发送线程，重复调用无副作用。
        for client in clients:
            client.close()

    print(f"{len(clients)} 架飞机各上报 {frames} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
