#!/usr/bin/env python3
"""连接 FlyRuler 服务端并打印会话标识。

用法：

    uv run python examples/01_connect.py
    uv run python examples/01_connect.py --aircraft Probe --heartbeat 2

脚本只做一次握手就正常关闭，用来确认服务端可达、两端协议版本一致。
服务端没有运行时握手会在约 1 秒后超时，脚本打印排障提示并以退出码 1 结束。
"""

from __future__ import annotations

import argparse

from fly_ruler_proto_python import (
    PROTOCOL_VERSION,
    FlyRulerClient,
    get_protocol_version,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="连接服务端并打印会话标识")
    parser.add_argument("--address", default="127.0.0.1:18002", help="服务端 UDP 地址")
    parser.add_argument("--aircraft", default="Probe", help="飞机显示名")
    parser.add_argument("--heartbeat", type=float, default=1.0, help="心跳间隔 [s]")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    print(f"PROTOCOL_VERSION={PROTOCOL_VERSION}")
    print(f"get_protocol_version()={get_protocol_version()}")
    print(f"连接 {args.address} ...")

    try:
        # 构造客户端即完成握手与注册，断开地址不可达时抛 ConnectionError。
        with FlyRulerClient(
            args.address,
            args.aircraft,
            heartbeat_interval_secs=args.heartbeat,
        ) as client:
            print(f"client_uuid={client.client_uuid}")
            print(f"aircraft_uuid={client.aircraft_uuid}")
    except ConnectionError as error:
        print(f"连接失败：{error}")
        print("确认服务端已启动，并检查地址与协议版本是否一致。")
        return 1

    print("连接已关闭。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
