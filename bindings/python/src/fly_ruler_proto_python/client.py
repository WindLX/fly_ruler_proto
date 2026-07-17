"""Aircraft-oriented Python wrapper over the Rust PyClient."""

from __future__ import annotations

import math
from collections.abc import Sequence

from fly_ruler_proto_python._core import (
    AircraftState,
    Attitude,
    ControlSurfaceState,
    DerivedState,
    PropulsorState,
    PyClient,
    TelemetryStreamSchema,
    Vector3,
)


def _validate_timestamp(name: str, timestamp: float | None) -> None:
    if timestamp is not None and not math.isfinite(timestamp):
        raise ValueError(f"{name} must be finite")


def create_aircraft_state(
    position: tuple[float, float, float] = (0.0, 0.0, 0.0),
    velocity: tuple[float, float, float] = (0.0, 0.0, 0.0),
    attitude: Attitude | None = None,
    angular_velocity: tuple[float, float, float] = (0.0, 0.0, 0.0),
    derived: DerivedState | None = None,
    control_surfaces: ControlSurfaceState | None = None,
    linear_acceleration_body: tuple[float, float, float] | None = None,
    propulsors: list[PropulsorState] | None = None,
) -> AircraftState:
    """Create a default AircraftState for convenient scripting."""
    return AircraftState(
        position=Vector3(*position),
        velocity=Vector3(*velocity),
        attitude=attitude or Attitude.identity(),
        angular_velocity=Vector3(*angular_velocity),
        derived=derived,
        control_surfaces=control_surfaces,
        linear_acceleration_body=(
            Vector3(*linear_acceleration_body)
            if linear_acceleration_body is not None
            else None
        ),
        propulsors=propulsors,
    )


class FlyRulerClient:
    """One client bound to one aircraft lifecycle.

    Example:
        with FlyRulerClient("127.0.0.1:18002", "F-16") as aircraft:
            aircraft.update_state(create_aircraft_state(position=(100.0, 0.0, -1000.0)))
            aircraft.create_event("missile_launch")

    Behavior:
        - constructor auto-connects + handshakes + spawns one aircraft
        - close()/context-exit auto-despawns and closes network
    """

    def __init__(
        self,
        address: str,
        aircraft_name: str,
        initial_state: AircraftState | None = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
        telemetry_schemas: Sequence[TelemetryStreamSchema] = (),
        spawn_timestamp: float | None = None,
    ) -> None:
        _validate_timestamp("spawn_timestamp", spawn_timestamp)
        if not math.isfinite(heartbeat_interval_secs) or heartbeat_interval_secs <= 0.0:
            raise ValueError(
                "heartbeat_interval_secs must be finite and greater than zero"
            )
        self._inner = PyClient(
            address,
            aircraft_name,
            initial_state or create_aircraft_state(),
            toml_config,
            heartbeat_interval_secs,
            list(telemetry_schemas),
            spawn_timestamp,
        )
        self._closed = False

    @property
    def client_uuid(self) -> str:
        return self._inner.client_uuid()

    @property
    def aircraft_uuid(self) -> str:
        return self._inner.aircraft_uuid()

    def update_state(
        self,
        state: AircraftState,
        timestamp: float | None = None,
    ) -> None:
        _validate_timestamp("timestamp", timestamp)
        self._inner.update_state(state, timestamp)

    def create_event(
        self,
        event_name: str,
        timestamp: float | None = None,
    ) -> None:
        _validate_timestamp("timestamp", timestamp)
        self._inner.create_event(event_name, timestamp)

    def publish_telemetry(
        self,
        stream_id: str,
        values: Sequence[float | int | bool],
        timestamp: float | None = None,
    ) -> None:
        """Publish one frame in the exact field order declared by the stream schema."""
        _validate_timestamp("timestamp", timestamp)
        self._inner.publish_telemetry(stream_id, tuple(values), timestamp)

    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None:
        _validate_timestamp("timestamp", timestamp)
        self._inner.despawn(reason, timestamp)

    def close(self) -> None:
        if not self._closed:
            self._inner.close()
            self._closed = True

    def __enter__(self) -> "FlyRulerClient":
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass
