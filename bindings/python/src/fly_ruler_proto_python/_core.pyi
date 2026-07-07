"""Type stubs for fly_ruler_proto_python._core."""

CustomFieldValue = float | int | bool | str | bytes

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

class ControlSurfaceState:
    alieron_left_rad: float | None
    alieron_right_rad: float | None
    elevator_rad: float | None
    rudder_rad: float | None
    flap_left_rad: float | None
    flap_right_rad: float | None
    spoiler_ratio: float | None

    def __init__(
        self,
        aileron_left_rad: float | None = None,
        aileron_right_rad: float | None = None,
        elevator_rad: float | None = None,
        rudder_rad: float | None = None,
        flap_left_rad: float | None = None,
        flap_right_rad: float | None = None,
        spoiler_ratio: float | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class EngineState:
    index: int
    throttle_lever_ratio: float | None

    def __init__(
        self, index: int, throttle_lever_ratio: float | None = None
    ) -> None: ...
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
    ias: float | None
    cas: float | None
    mach: float | None

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
        ias: float | None = None,
        cas: float | None = None,
        mach: float | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class AircraftState:
    position: Vector3
    velocity: Vector3
    attitude: Quaternion
    angular_velocity: Vector3
    derived: DerivedState | None
    control_surfaces: ControlSurfaceState | None
    engines: list[EngineState]
    custom_fields: dict[str, CustomFieldValue]

    def __init__(
        self,
        position: Vector3 | None = None,
        velocity: Vector3 | None = None,
        attitude: Quaternion | None = None,
        angular_velocity: Vector3 | None = None,
        derived: DerivedState | None = None,
        control_surfaces: ControlSurfaceState | None = None,
        engines: list[EngineState] | None = None,
        custom_fields: dict[str, CustomFieldValue] | None = None,
    ) -> None: ...
    @staticmethod
    def hover() -> "AircraftState": ...
    def set_custom_field(self, field_id: str, value: CustomFieldValue) -> None: ...
    def __repr__(self) -> str: ...

class PyClient:
    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: AircraftState | None = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
    ) -> None: ...
    def client_uuid(self) -> str: ...
    def aircraft_uuid(self) -> str: ...
    def update_state(
        self, state: AircraftState, timestamp: float | None = None
    ) -> None: ...
    def create_event(self, event_name: str, timestamp: float | None = None) -> None: ...
    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None: ...
    def close(self) -> None: ...
