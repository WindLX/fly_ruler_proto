#!/usr/bin/env python3
"""连接服务端、打印会话标识并按固定频率推送状态。

对应使用手册 `docs/guide/02-quickstart.md`。

用法：

    uv run python examples/01_connect_and_push.py
    uv run python examples/01_connect_and_push.py --hz 20 --duration 10 --speed 120
    uv run python examples/01_connect_and_push.py --duration 0

前置条件：本机已按 `README.md` 在 ``bindings/python`` 下装好客户端库，并且有一个
服务端在 ``--address``（默认 ``127.0.0.1:18002``）上收 UDP 报文；独立服务端用
``just dev server`` 启动，想喂给模拟器就改起 MSFS 桥接 ``just msfs run``。本示例
只发状态、不发控制面与发动机，因此没有模拟器时同样能跑。

脚本先完成握手，打印协议版本与两个会话 UUID，随后飞机沿机体前方匀速直线飞行，
同时以固定爬升率上升，按 ``--hz``（默认 30）上报状态，持续 ``--duration`` 秒；
``--duration 0`` 表示一直上报到中断。每一帧都用 ``create_aircraft_state`` 重新
构造状态快照，再交给 ``update_state``，时间戳取 ``time.time()``，服务端按这个
值写入时间序列。

跑通后终端先打印 ``PROTOCOL_VERSION`` 与 ``get_protocol_version()``，两者一致说明
两端协议版本相同；接着打印 ``client_uuid``（这条连接）与 ``aircraft_uuid``（这架
飞机），随后每秒打印一次已上报帧数。打开控制台 ``http://127.0.0.1:18003`` 可以在
左栏看到 ``--aircraft`` 指定的飞机与它的状态曲线；机体系速度固定指向机体前方，
因此水平面的位置曲线是一条沿 ``--heading`` 方向的直线。

服务端不可达时打印中文原因并以退出码 1 结束；``Ctrl-C`` 会走完 ``finally``
关闭客户端后退出。
"""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import (
    PROTOCOL_VERSION,
    Attitude,
    FlyRulerClient,
    create_aircraft_state,
    get_protocol_version,
)


@dataclass(frozen=True)
class FlightConfig:
    """直线飞行的运动参数。"""

    speed_mps: float
    climb_mps: float
    heading_rad: float


def build_state(elapsed_s: float, config: FlightConfig):
    """按经过时间构造状态快照。"""
    # 把飞行时间换算成 NED 平面上的位移：北向与东向按航向角分解。
    forward_m = config.speed_mps * elapsed_s
    north_m = forward_m * math.cos(config.heading_rad)
    east_m = forward_m * math.sin(config.heading_rad)
    altitude_m = 800.0 + config.climb_mps * elapsed_s

    # 航向角绕机体 Z 轴，四元数只含偏航分量。
    half = config.heading_rad * 0.5
    quaternion = (math.cos(half), 0.0, 0.0, math.sin(half))

    return create_aircraft_state(
        position=(north_m, east_m, -altitude_m),
        # 速度是机体系（x 前、y 右、z 下），平飞时只有前向与爬升分量。
        velocity=(config.speed_mps, 0.0, -config.climb_mps),
        attitude=Attitude.from_quaternion(quaternion),
        angular_velocity=(0.0, 0.0, 0.0),
    )


def parse_args() -> argparse.Namespace:
    # 参数与 README 的公共约定一致：地址、飞机名、频率、时长，另加本例的航迹参数。
    parser = argparse.ArgumentParser(description="连接服务端并推送飞机状态")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="Probe", help="飞机显示名")
    parser.add_argument("--hz", type=float, default=30.0, help="上报频率 [Hz]")
    parser.add_argument(
        "--duration", type=float, default=5.0, help="上报时长 [s]，0 表示一直上报"
    )
    parser.add_argument("--speed", type=float, default=90.0, help="地速 [m/s]")
    parser.add_argument("--climb", type=float, default=5.0, help="爬升率 [m/s]")
    parser.add_argument("--heading", type=float, default=45.0, help="航向角 [deg]")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    # 频率与时长先做本地校验，避免用非法参数连接服务端。
    if args.hz <= 0:
        raise SystemExit("--hz 必须大于 0")
    if args.duration < 0:
        raise SystemExit("--duration 不能为负")

    # 用自定义信号处理代替 KeyboardInterrupt：主循环每轮检查 running，
    # 这样退出时能走完 finally 里的关闭逻辑。
    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    # 命令行角度按度给，内部统一转成弧度参与三角函数计算。
    config = FlightConfig(
        speed_mps=args.speed,
        climb_mps=args.climb,
        heading_rad=math.radians(args.heading),
    )

    # 先打印本地协议版本常量，再打印运行时查询值，两者相同才说明两端一致。
    print(f"PROTOCOL_VERSION={PROTOCOL_VERSION}")
    print(f"get_protocol_version()={get_protocol_version()}")
    print(f"连接 {args.address} ...")

    try:
        # 构造客户端即完成握手与注册，地址不可达时抛 ConnectionError。
        client = FlyRulerClient(
            args.address,
            args.aircraft,
            initial_state=build_state(0.0, config),
        )
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动（just dev server），并检查地址与协议版本是否一致。")
        return 1

    print(f"client_uuid={client.client_uuid}")
    print(f"aircraft_uuid={client.aircraft_uuid}")

    # 用单调时钟排定每个发送时刻，避免 sleep 与系统时间调整带来的累计漂移。
    period = 1.0 / args.hz
    start = time.monotonic()
    next_tick = start
    sent = 0

    try:
        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break

            # timestamp 用墙上时钟 time.time()，服务端按它排序写入时间序列；
            # 这里省略 timestamp 参数效果相同。
            client.update_state(build_state(elapsed, config), timestamp=time.time())
            sent += 1

            # 每满一秒的帧数打一行进度，便于确认上报没有卡住。
            if sent % max(int(args.hz), 1) == 0:
                print(f"已上报 {sent} 帧，仿真时间 {elapsed:.1f} s")

            # 对齐到下一个周期；落后太多时直接重置基准，不做补偿追赶。
            next_tick += period
            delay = next_tick - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            else:
                next_tick = time.monotonic()
    finally:
        # 关闭连接会停止心跳与发送线程，重复调用无副作用。
        client.close()

    print(f"共上报 {sent} 帧，连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
