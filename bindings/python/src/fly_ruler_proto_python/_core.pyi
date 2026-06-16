"""Type stubs for fly_ruler_proto_python._core."""

from typing import Optional

PROTOCOL_VERSION: str

def get_protocol_version() -> str: ...

class Vector3:
    x: float
    y: float
    z: float

    def __init__(self, x: float, y: float, z: float) -> None: ...
    @staticmethod
    def zero() -> "Vector3": ...
    def __repr__(self) -> str: ...

class Quaternion:
    w: float
    x: float
    y: float
    z: float

    def __init__(self, w: float, x: float, y: float, z: float) -> None: ...
    @staticmethod
    def identity() -> "Quaternion": ...
    def __repr__(self) -> str: ...

class DerivedState:
    lat: float
    lon: float
    altitude: float
    alpha: float
    beta: float
    tas: float
    eas: float
    gamma: float
    chi: float

    def __init__(
        self,
        lat: float,
        lon: float,
        altitude: float,
        alpha: float = 0.0,
        beta: float = 0.0,
        tas: float = 0.0,
        eas: float = 0.0,
        gamma: float = 0.0,
        chi: float = 0.0,
    ) -> None: ...
    def __repr__(self) -> str: ...

class AircraftState:
    position: Vector3
    velocity: Vector3
    attitude: Quaternion
    angular_velocity: Vector3
    derived: Optional[DerivedState]

    def __init__(
        self,
        position: Optional[Vector3] = None,
        velocity: Optional[Vector3] = None,
        attitude: Optional[Quaternion] = None,
        angular_velocity: Optional[Vector3] = None,
        derived: Optional[DerivedState] = None,
    ) -> None: ...

    @staticmethod
    def hover() -> "AircraftState": ...
    def __repr__(self) -> str: ...

class PyClient:
    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: Optional[AircraftState] = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
    ) -> None: ...
    def client_uuid(self) -> str: ...
    def aircraft_uuid(self) -> str: ...
    def update_state(self, state: AircraftState, timestamp: Optional[float] = None) -> None: ...
    def create_event(self, event_name: str, timestamp: Optional[float] = None) -> None: ...
    def despawn(self, reason: Optional[str] = None, timestamp: Optional[float] = None) -> None: ...
    def close(self) -> None: ...

class PyServer:
    def __init__(self, addr: str) -> None: ...
    def local_addr(self) -> str: ...
    def close(self) -> None: ...
