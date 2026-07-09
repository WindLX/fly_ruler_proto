#!/usr/bin/env python3
"""Send multiple FlyRuler aircraft for MSFS user+AI rendering tests.

The MSFS bridge maps one aircraft to the user aircraft and, when started with
``--enable-ai-aircraft``, maps the other spawned aircraft to AI visual aircraft.
This script can either launch several aircraft from one process, or launch one
indexed aircraft so multiple independent processes can be combined.
"""

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
class AircraftDemo:
    index: int
    total: int
    latitude_deg: float
    longitude_deg: float
    altitude_m: float
    radius_m: float
    speed_mps: float
    engine_count: int
    phase_offset_rad: float


def build_state(elapsed_s: float, demo: AircraftDemo):
    omega = demo.speed_mps / max(demo.radius_m, 1e-6)
    phase = omega * elapsed_s + demo.phase_offset_rad

    # Separate each aircraft slightly in altitude/radius so AI aircraft are
    # visible even before phase offsets spread them around the circle.
    altitude = demo.altitude_m + demo.index * 45.0
    radius = demo.radius_m + demo.index * 80.0

    north_m = radius * math.cos(phase)
    east_m = radius * math.sin(phase)
    latitude = demo.latitude_deg + math.degrees(north_m / EARTH_RADIUS_M)
    longitude = demo.longitude_deg + math.degrees(
        east_m / (EARTH_RADIUS_M * max(math.cos(math.radians(demo.latitude_deg)), 1e-6))
    )

    velocity_north = -radius * omega * math.sin(phase)
    velocity_east = radius * omega * math.cos(phase)
    yaw = math.atan2(velocity_east, velocity_north)
    quaternion = (math.cos(yaw * 0.5), 0.0, 0.0, math.sin(yaw * 0.5))

    bank = 0.18 * math.sin(phase * 0.7 + demo.index)
    control_phase = math.sin(phase + demo.index * 0.3)
    throttle = 0.55 + 0.12 * math.sin(phase * 0.25 + demo.index)

    return create_aircraft_state(
        position=(north_m, east_m, -altitude),
        # Body-FRD velocity: x forward, y right, z down. The demo flies level.
        velocity=(demo.speed_mps, 0.0, 0.0),
        attitude=quaternion,
        angular_velocity=(0.0, 0.0, omega),
        derived=DerivedState(
            lat=latitude,
            lon=longitude,
            altitude=altitude,
            tas=demo.speed_mps,
            eas=demo.speed_mps,
            chi=yaw,
        ),
        control_surfaces=ControlSurfaceState(
            aileron_left_rad=bank + 0.08 * control_phase,
            aileron_right_rad=-(bank + 0.08 * control_phase),
            elevator_rad=0.04 * math.sin(phase * 0.5),
            rudder_rad=0.05 * math.cos(phase),
            flaps_left_ratio=0.0,
            flaps_right_ratio=0.0,
            spoilers_ratio=0.0,
        ),
        engines=[
            EngineState(index, throttle_lever_ratio=throttle)
            for index in range(1, demo.engine_count + 1)
        ],
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="127.0.0.1:18002")
    parser.add_argument("--name-prefix", default="FlyRulerMSFSAI")
    parser.add_argument("--latitude", type=float, default=31.1434)
    parser.add_argument("--longitude", type=float, default=121.8052)
    parser.add_argument("--altitude", type=float, default=800.0)
    parser.add_argument("--radius", type=float, default=500.0)
    parser.add_argument("--speed", type=float, default=70.0)
    parser.add_argument("--hz", type=float, default=60.0)
    parser.add_argument("--engine-count", type=int, choices=range(1, 5), default=2)
    parser.add_argument("--duration", type=float, default=0.0)
    parser.add_argument(
        "--aircraft-count",
        type=int,
        default=3,
        help="How many aircraft to spawn from this process unless --aircraft-index is set",
    )
    parser.add_argument(
        "--aircraft-index",
        type=int,
        default=None,
        help="Spawn only one indexed aircraft; useful when running multiple client processes",
    )
    parser.add_argument(
        "--gear-cycle-secs",
        type=float,
        default=0.0,
        help="Alternate gear up/down events at this interval; <=0 disables",
    )
    return parser.parse_args()


def demos_from_args(args: argparse.Namespace) -> list[AircraftDemo]:
    if args.aircraft_count <= 0:
        raise ValueError("--aircraft-count must be greater than zero")
    if args.hz <= 0:
        raise ValueError("--hz must be greater than zero")

    indices = (
        [args.aircraft_index]
        if args.aircraft_index is not None
        else list(range(args.aircraft_count))
    )
    demos = []
    for index in indices:
        if index < 0 or index >= args.aircraft_count:
            raise ValueError("--aircraft-index must be in 0..aircraft-count-1")
        demos.append(
            AircraftDemo(
                index=index,
                total=args.aircraft_count,
                latitude_deg=args.latitude,
                longitude_deg=args.longitude,
                altitude_m=args.altitude,
                radius_m=args.radius,
                speed_mps=args.speed,
                engine_count=args.engine_count,
                phase_offset_rad=2.0 * math.pi * index / max(args.aircraft_count, 1),
            )
        )
    return demos


def main() -> int:
    args = parse_args()
    demos = demos_from_args(args)
    period = 1.0 / args.hz
    running = True

    def stop(_signal, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    clients: list[tuple[FlyRulerClient, AircraftDemo]] = []
    try:
        for demo in demos:
            initial_state = build_state(0.0, demo)
            client = FlyRulerClient(
                args.address,
                f"{args.name_prefix}-{demo.index + 1}",
                initial_state=initial_state,
                toml_config=(
                    "[aircraft]\n"
                    "model='msfs_ai_visual_demo'\n"
                    f"formation_index={demo.index}\n"
                ),
            )
            clients.append((client, demo))
            role = (
                "candidate user aircraft" if demo.index == 0 else "AI visual aircraft"
            )
            print(
                f"{args.name_prefix}-{demo.index + 1}: "
                f"aircraft_uuid={client.aircraft_uuid} ({role})"
            )

        print(
            f"Sending {len(clients)} aircraft to {args.address} at {args.hz:g} Hz. "
            "Start the MSFS bridge with --enable-ai-aircraft to render non-user aircraft."
        )
        print(
            "If you want a specific main aircraft, pass its printed UUID to the bridge "
            "with --aircraft-id."
        )

        start = time.monotonic()
        next_tick = start
        next_gear_event = args.gear_cycle_secs
        gear_down = True
        while running:
            elapsed = time.monotonic() - start
            if args.duration > 0 and elapsed >= args.duration:
                break

            timestamp = time.time()
            for client, demo in clients:
                client.update_state(build_state(elapsed, demo), timestamp=timestamp)

            if args.gear_cycle_secs > 0 and elapsed >= next_gear_event:
                gear_down = not gear_down
                event_name = (
                    "flyruler.control.gear_down"
                    if gear_down
                    else "flyruler.control.gear_up"
                )
                for client, _demo in clients:
                    client.create_event(event_name, timestamp=time.time())
                print(f"sent {event_name} to {len(clients)} aircraft")
                next_gear_event += args.gear_cycle_secs

            next_tick += period
            delay = next_tick - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            else:
                next_tick = time.monotonic()
    finally:
        for client, _demo in reversed(clients):
            client.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
