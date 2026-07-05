#!/usr/bin/env python3
"""Drive the MSFS bridge around a small geodetic circle."""

from __future__ import annotations

import argparse
import math
import signal
import time
from dataclasses import dataclass

from fly_ruler_proto_python import (
    ControlSurfaceState,
    DerivedState,
    EngineState,
    FlyRulerClient,
    create_aircraft_state,
)

EARTH_RADIUS_M = 6_378_137.0


@dataclass(frozen=True)
class DemoConfig:
    latitude_deg: float
    longitude_deg: float
    altitude_m: float
    radius_m: float
    speed_mps: float
    engine_count: int


def build_state(elapsed_s: float, config: DemoConfig):
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
    quaternion = (math.cos(yaw * 0.5), 0.0, 0.0, math.sin(yaw * 0.5))

    control_phase = math.sin(phase)
    controls = {
        "flyruler.control.aileron_left_rad": 0.12 * control_phase,
        "flyruler.control.aileron_right_rad": -0.12 * control_phase,
        "flyruler.control.elevator_rad": 0.05 * math.sin(phase * 0.5),
        "flyruler.control.rudder_rad": 0.08 * math.cos(phase),
        "flyruler.control.flaps_left_ratio": 0.0,
        "flyruler.control.flaps_right_ratio": 0.0,
        "flyruler.control.spoilers_ratio": 0.0,
    }
    throttle = 0.55 + 0.15 * math.sin(phase * 0.25)

    return create_aircraft_state(
        position=(north_m, east_m, -config.altitude_m),
        velocity=(velocity_north, velocity_east, 0.0),
        attitude=quaternion,
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
            aileron_left_rad=controls["flyruler.control.aileron_left_rad"],
            aileron_right_rad=controls["flyruler.control.aileron_right_rad"],
            elevator_rad=controls["flyruler.control.elevator_rad"],
            rudder_rad=controls["flyruler.control.rudder_rad"],
            flaps_left_ratio=0.0,
            flaps_right_ratio=0.0,
            spoilers_ratio=0.0,
        ),
        engines=[
            EngineState(index, throttle_lever_ratio=throttle)
            for index in range(1, config.engine_count + 1)
        ],
        custom_fields=controls,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="127.0.0.1:18002")
    parser.add_argument("--name", default="FlyRulerMSFSDemo")
    parser.add_argument("--latitude", type=float, default=31.1434)
    parser.add_argument("--longitude", type=float, default=121.8052)
    parser.add_argument("--altitude", type=float, default=1200.0)
    parser.add_argument("--radius", type=float, default=500.0)
    parser.add_argument("--speed", type=float, default=70.0)
    parser.add_argument("--hz", type=float, default=60.0)
    parser.add_argument("--engine-count", type=int, choices=range(1, 5), default=2)
    parser.add_argument("--duration", type=float, default=0.0)
    parser.add_argument(
        "--gear-cycle-secs",
        type=float,
        default=0.0,
        help="Alternate gear up/down events at this interval; <=0 disables",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.hz <= 0:
        raise ValueError("--hz must be greater than zero")

    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    config = DemoConfig(
        latitude_deg=args.latitude,
        longitude_deg=args.longitude,
        altitude_m=args.altitude,
        radius_m=args.radius,
        speed_mps=args.speed,
        engine_count=args.engine_count,
    )
    initial_state = build_state(0.0, config)
    period = 1.0 / args.hz

    print(f"Sending MSFS visual state to {args.address} at {args.hz:g} Hz")
    with FlyRulerClient(
        args.address,
        args.name,
        initial_state=initial_state,
        toml_config="[aircraft]\nmodel='msfs_visual_demo'",
    ) as client:
        print(f"aircraft_uuid={client.aircraft_uuid}")
        start = time.monotonic()
        next_tick = start
        next_gear_event = args.gear_cycle_secs
        gear_down = True
        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break
            client.update_state(build_state(elapsed, config), timestamp=time.time())
            if args.gear_cycle_secs > 0 and elapsed >= next_gear_event:
                gear_down = not gear_down
                event_name = (
                    "flyruler.control.gear_down"
                    if gear_down
                    else "flyruler.control.gear_up"
                )
                client.create_event(event_name, timestamp=time.time())
                print(f"sent {event_name}")
                next_gear_event += args.gear_cycle_secs
            next_tick += period
            delay = next_tick - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            else:
                next_tick = time.monotonic()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
