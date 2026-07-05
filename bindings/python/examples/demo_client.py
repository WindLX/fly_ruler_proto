#!/usr/bin/env python3
"""Example FlyRuler Python client for Godot integration tests.

This script simulates one aircraft flying a horizontal circle and periodically
sending custom events. It is intended for end-to-end testing with the
FlyRuler Godot GDExtension demo.
"""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import FlyRulerClient, create_aircraft_state


@dataclass
class MotionConfig:
    radius_m: float
    speed_mps: float
    altitude_m: float


def build_state(elapsed_s: float, cfg: MotionConfig):
    # Uniform circular motion in XY plane, negative Z for NED-like "up" altitude.
    omega = cfg.speed_mps / max(cfg.radius_m, 1e-6)
    theta = omega * elapsed_s

    x = cfg.radius_m * math.cos(theta)
    y = cfg.radius_m * math.sin(theta)
    z = -cfg.altitude_m

    vx = -cfg.radius_m * omega * math.sin(theta)
    vy = cfg.radius_m * omega * math.cos(theta)
    vz = 0.0

    # Yaw-only attitude from velocity direction.
    yaw = math.atan2(vy, vx)
    qw = math.cos(yaw * 0.5)
    qz = math.sin(yaw * 0.5)

    return create_aircraft_state(
        position=(x, y, z),
        velocity=(vx, vy, vz),
        attitude=(qw, 0.0, 0.0, qz),
        angular_velocity=(0.0, 0.0, omega),
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="FlyRuler demo simulator client")
    parser.add_argument(
        "--address", default="127.0.0.1:18002", help="FlyRuler UDP server address"
    )
    parser.add_argument("--name", default="DemoAircraft", help="Aircraft display name")
    parser.add_argument("--hz", type=float, default=30.0, help="State update frequency")
    parser.add_argument(
        "--duration",
        type=float,
        default=0.0,
        help="Run duration seconds, 0 means run forever",
    )
    parser.add_argument(
        "--radius", type=float, default=300.0, help="Circle radius in meters"
    )
    parser.add_argument("--speed", type=float, default=60.0, help="Speed in m/s")
    parser.add_argument(
        "--altitude", type=float, default=1200.0, help="Altitude in meters"
    )
    parser.add_argument(
        "--event-every",
        type=float,
        default=5.0,
        help="Custom event interval seconds, <=0 disables",
    )
    parser.add_argument(
        "--toml-config",
        default="[aircraft]\nmodel='demo'\nrole='test_sender'",
        help="TOML config payload sent during spawn",
    )
    parser.add_argument(
        "--heartbeat", type=float, default=1.0, help="Heartbeat interval in seconds"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    if args.hz <= 0:
        raise ValueError("--hz must be > 0")

    running = True

    def _stop(_sig, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, _stop)
    signal.signal(signal.SIGTERM, _stop)

    cfg = MotionConfig(
        radius_m=args.radius,
        speed_mps=args.speed,
        altitude_m=args.altitude,
    )

    initial_state = build_state(0.0, cfg)

    print(f"Connecting to {args.address} as {args.name} ...")
    with FlyRulerClient(
        args.address,
        args.name,
        initial_state=initial_state,
        toml_config=args.toml_config,
        heartbeat_interval_secs=args.heartbeat,
    ) as client:
        print(
            f"Connected. client_uuid={client.client_uuid} aircraft_uuid={client.aircraft_uuid}"
        )

        period = 1.0 / args.hz
        start_monotonic = time.monotonic()
        next_tick = start_monotonic
        last_event_t = 0.0

        while running:
            now_monotonic = time.monotonic()
            elapsed = now_monotonic - start_monotonic

            if args.duration > 0 and elapsed >= args.duration:
                break

            state = build_state(elapsed, cfg)
            wall_ts = time.time()
            client.update_state(state, timestamp=wall_ts)

            if args.event_every > 0 and (elapsed - last_event_t) >= args.event_every:
                client.create_event("demo_tick", timestamp=wall_ts)
                last_event_t = elapsed

            next_tick += period
            sleep_s = next_tick - time.monotonic()
            if sleep_s > 0:
                time.sleep(sleep_s)
            else:
                next_tick = time.monotonic()

        client.create_event("demo_finished", timestamp=time.time())

    print("Client closed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
