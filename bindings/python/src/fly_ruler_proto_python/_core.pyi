"""Type stubs for fly_ruler_proto_python._core."""

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

class Attitude:
    @staticmethod
    def identity() -> "Attitude": ...
    @staticmethod
    def from_quaternion(values: list[float] | tuple[float, ...]) -> "Attitude": ...
    @staticmethod
    def from_rotation_matrix(
        values: list[float] | tuple[float, ...],
    ) -> "Attitude": ...
    @staticmethod
    def from_euler(values: list[float] | tuple[float, ...]) -> "Attitude": ...
    @property
    def quaternion(self) -> tuple[float, float, float, float]: ...
    @property
    def rotation_matrix(
        self,
    ) -> tuple[float, float, float, float, float, float, float, float, float]: ...
    @property
    def euler(self) -> tuple[float, float, float]: ...
    def __repr__(self) -> str: ...

class ControlSurfaceState:
    aileron_left_rad: float | None
    aileron_right_rad: float | None
    elevator_rad: float | None
    rudder_rad: float | None
    flaps_left_ratio: float | None
    flaps_right_ratio: float | None
    spoilers_ratio: float | None

    def __init__(
        self,
        aileron_left_rad: float | None = None,
        aileron_right_rad: float | None = None,
        elevator_rad: float | None = None,
        rudder_rad: float | None = None,
        flaps_left_ratio: float | None = None,
        flaps_right_ratio: float | None = None,
        spoilers_ratio: float | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PropulsorState:
    propulsor_id: str
    kind: int
    throttle_ratio: float | None
    rpm: float | None
    blade_pitch_rad: float | None
    thrust_newton: float | None
    torque_newton_meter: float | None
    index: int | None
    def __init__(
        self,
        propulsor_id: str,
        kind: int = 0,
        throttle_ratio: float | None = None,
        rpm: float | None = None,
        blade_pitch_rad: float | None = None,
        thrust_newton: float | None = None,
        torque_newton_meter: float | None = None,
        index: int | None = None,
    ) -> None: ...

class TelemetryValueType:
    F64: "TelemetryValueType"
    I64: "TelemetryValueType"
    Bool: "TelemetryValueType"

class TelemetryField:
    field_id: str
    label: str
    group: str
    unit: str
    description: str
    value_type: TelemetryValueType
    def __init__(
        self,
        field_id: str,
        label: str = "",
        group: str = "",
        unit: str = "",
        description: str = "",
        value_type: TelemetryValueType = TelemetryValueType.F64,
    ) -> None: ...

class TelemetryStreamSchema:
    stream_id: str
    name: str
    nominal_rate_hz: float | None
    fields: list[TelemetryField]
    def __init__(
        self,
        stream_id: str,
        fields: list[TelemetryField],
        name: str = "",
        nominal_rate_hz: float | None = None,
    ) -> None: ...

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
    ground_speed: float | None
    vertical_speed: float | None
    dynamic_pressure: float | None
    normal_load_factor: float | None

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
        ground_speed: float | None = None,
        vertical_speed: float | None = None,
        dynamic_pressure: float | None = None,
        normal_load_factor: float | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class AircraftState:
    position: Vector3
    velocity: Vector3
    attitude: Attitude
    angular_velocity: Vector3
    derived: DerivedState | None
    control_surfaces: ControlSurfaceState | None
    linear_acceleration_body: Vector3 | None
    propulsors: list[PropulsorState]

    def __init__(
        self,
        position: Vector3 | None = None,
        velocity: Vector3 | None = None,
        attitude: Attitude | None = None,
        angular_velocity: Vector3 | None = None,
        derived: DerivedState | None = None,
        control_surfaces: ControlSurfaceState | None = None,
        linear_acceleration_body: Vector3 | None = None,
        propulsors: list[PropulsorState] | None = None,
    ) -> None: ...
    @staticmethod
    def hover() -> "AircraftState": ...
    def __repr__(self) -> str: ...

class PyClient:
    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: AircraftState | None = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
        telemetry_schemas: list[TelemetryStreamSchema] | None = None,
        spawn_timestamp: float | None = None,
    ) -> None: ...
    def client_uuid(self) -> str: ...
    def aircraft_uuid(self) -> str: ...
    def update_state(
        self, state: AircraftState, timestamp: float | None = None
    ) -> None: ...
    def create_event(self, event_name: str, timestamp: float | None = None) -> None: ...
    def publish_telemetry(
        self,
        stream_id: str,
        values: tuple[float | int | bool, ...],
        timestamp: float | None = None,
    ) -> None: ...
    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None: ...
    def close(self) -> None: ...
