"""Aircraft-oriented Python wrapper over the Rust PyClient."""

from __future__ import annotations

from typing import Mapping

from fly_ruler_proto_python._core import (
    AircraftState,
    ControlSurfaceState,
    DerivedState,
    EngineState,
    PyClient,
    Quaternion,
    Vector3,
)


def create_aircraft_state(
    position: tuple[float, float, float] = (0.0, 0.0, 0.0),
    velocity: tuple[float, float, float] = (0.0, 0.0, 0.0),
    attitude: tuple[float, float, float, float] = (1.0, 0.0, 0.0, 0.0),
    angular_velocity: tuple[float, float, float] = (0.0, 0.0, 0.0),
    derived: DerivedState | None = None,
    control_surfaces: ControlSurfaceState | None = None,
    engines: list[EngineState] | None = None,
    custom_fields: Mapping[str, float | int | bool | str | bytes] | None = None,
) -> AircraftState:
    """Create a default AircraftState for convenient scripting."""
    return AircraftState(
        position=Vector3(*position),
        velocity=Vector3(*velocity),
        attitude=Quaternion(*attitude),
        angular_velocity=Vector3(*angular_velocity),
        derived=derived,
        control_surfaces=control_surfaces,
        engines=engines,
        custom_fields=custom_fields,
    )


class FlyRulerClient:
    """One client bound to one aircraft lifecycle.

    Example:
        with FlyRulerClient("127.0.0.1:8080", "F-16") as aircraft:
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
    ) -> None:
        self._inner = PyClient(
            address,
            aircraft_name,
            initial_state or create_aircraft_state(),
            toml_config,
            heartbeat_interval_secs,
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
        self._inner.update_state(state, timestamp)

    def create_event(
        self,
        event_name: str,
        timestamp: float | None = None,
    ) -> None:
        self._inner.create_event(event_name, timestamp)

    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None:
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
