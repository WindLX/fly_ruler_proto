"""``fly_ruler_proto_python._core`` Rust 扩展的类型存根。"""

PROTOCOL_VERSION: str
"""协议版本字符串，与 Rust 核心导出的一致；握手时按严格相等比较。"""

def get_protocol_version() -> str:
    """返回当前协议版本字符串。

    Returns:
        与 ``PROTOCOL_VERSION`` 相同的语义化版本字符串。
    """

class Vector3:
    """三维向量，使用 SI 单位。

    Attributes:
        x: X 分量。
        y: Y 分量。
        z: Z 分量。
    """

    x: float
    y: float
    z: float

    def __init__(self, x: float, y: float, z: float) -> None:
        """初始化三维向量。

        Args:
            x: X 分量。
            y: Y 分量。
            z: Z 分量。
        """
    @staticmethod
    def zero() -> "Vector3":
        """返回各分量均为零的向量。

        Returns:
            新的零向量实例。
        """
    def __repr__(self) -> str:
        """返回各分量的可读表示。

        Returns:
            调试用字符串。
        """

class Attitude:
    """姿态，内部以 scalar-first 四元数存储。

    提供四元数、旋转矩阵与欧拉角三种视图，角度量均为弧度。
    """

    @staticmethod
    def identity() -> "Attitude":
        """返回单位姿态。

        Returns:
            对应单位四元数的新姿态实例。
        """
    @staticmethod
    def from_quaternion(values: list[float] | tuple[float, ...]) -> "Attitude":
        """由四元数构造姿态。

        Args:
            values: 按 scalar-first 顺序排列的分量 ``(w, x, y, z)``。

        Returns:
            新的姿态实例。
        """
    @staticmethod
    def from_rotation_matrix(
        values: list[float] | tuple[float, ...],
    ) -> "Attitude":
        """由行优先旋转矩阵构造姿态。

        Args:
            values: 9 个按行优先排列的旋转矩阵元素。

        Returns:
            新的姿态实例。
        """
    @staticmethod
    def from_euler(values: list[float] | tuple[float, ...]) -> "Attitude":
        """由欧拉角构造姿态。

        Args:
            values: 欧拉角三元组，单位 ``[rad]``。

        Returns:
            新的姿态实例。
        """
    @property
    def quaternion(self) -> tuple[float, float, float, float]:
        """返回 scalar-first 四元数 ``(w, x, y, z)``。

        Returns:
            四元数分量元组。
        """
    @property
    def rotation_matrix(
        self,
    ) -> tuple[float, float, float, float, float, float, float, float, float]:
        """返回行优先旋转矩阵。

        Returns:
            9 个按行优先排列的矩阵元素。
        """
    @property
    def euler(self) -> tuple[float, float, float]:
        """返回欧拉角三元组。

        Returns:
            欧拉角 ``[rad]``。
        """
    def __repr__(self) -> str:
        """返回姿态的可读表示。

        Returns:
            调试用字符串。
        """

class ControlSurfaceState:
    """可选的控制面偏度与襟翼/扰流板比例。

    Attributes:
        aileron_left_rad: 左副翼偏角 ``[rad]``，``None`` 表示未提供。
        aileron_right_rad: 右副翼偏角 ``[rad]``，``None`` 表示未提供。
        elevator_rad: 升降舵偏角 ``[rad]``，``None`` 表示未提供。
        rudder_rad: 方向舵偏角 ``[rad]``，``None`` 表示未提供。
        flaps_left_ratio: 左襟翼比例，``None`` 表示未提供。
        flaps_right_ratio: 右襟翼比例，``None`` 表示未提供。
        spoilers_ratio: 扰流板比例，``None`` 表示未提供。
    """

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
    ) -> None:
        """初始化控制面状态，所有字段均可省略。

        Args:
            aileron_left_rad: 左副翼偏角 ``[rad]``。
            aileron_right_rad: 右副翼偏角 ``[rad]``。
            elevator_rad: 升降舵偏角 ``[rad]``。
            rudder_rad: 方向舵偏角 ``[rad]``。
            flaps_left_ratio: 左襟翼比例。
            flaps_right_ratio: 右襟翼比例。
            spoilers_ratio: 扰流板比例。
        """
    def __repr__(self) -> str:
        """返回控制面状态的可读表示。

        Returns:
            调试用字符串。
        """

class PropulsorState:
    """单个推进器的状态。

    Attributes:
        propulsor_id: 推进器标识。
        kind: 推进器类别，取值对应 ``PropulsorKind``。
        throttle_ratio: 油门比例，``None`` 表示未提供。
        rpm: 转速 ``[rpm]``，``None`` 表示未提供。
        blade_pitch_rad: 桨叶桨距角 ``[rad]``，``None`` 表示未提供。
        thrust_newton: 推力 ``[N]``，``None`` 表示未提供。
        torque_newton_meter: 扭矩 ``[N*m]``，``None`` 表示未提供。
        index: 推进器索引，``None`` 表示未提供。
    """

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
    ) -> None:
        """初始化推进器状态。

        Args:
            propulsor_id: 推进器标识。
            kind: 推进器类别，默认 ``0`` 表示未指定。
            throttle_ratio: 油门比例。
            rpm: 转速 ``[rpm]``。
            blade_pitch_rad: 桨叶桨距角 ``[rad]``。
            thrust_newton: 推力 ``[N]``。
            torque_newton_meter: 扭矩 ``[N*m]``。
            index: 推进器索引。
        """

class TelemetryValueType:
    """遥测字段的取值类型标记。

    Attributes:
        F64: 双精度浮点值。
        I64: 64 位有符号整数值。
        Bool: 布尔值。
    """

    F64: "TelemetryValueType"
    I64: "TelemetryValueType"
    Bool: "TelemetryValueType"

class TelemetryField:
    """遥测流中的单个字段描述。

    Attributes:
        field_id: 字段标识。
        label: 展示用短标签。
        group: 字段分组名。
        unit: 单位字符串。
        description: 字段说明。
        value_type: 字段取值类型。
    """

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
    ) -> None:
        """初始化遥测字段描述。

        Args:
            field_id: 字段标识。
            label: 展示用短标签，默认为空串。
            group: 字段分组名，默认为空串。
            unit: 单位字符串，默认为空串。
            description: 字段说明，默认为空串。
            value_type: 字段取值类型，默认 ``TelemetryValueType.F64``。
        """

class TelemetryStreamSchema:
    """遥测流的结构声明。

    Attributes:
        stream_id: 流标识。
        name: 流名称。
        nominal_rate_hz: 标称发布频率 ``[Hz]``，``None`` 表示未提供。
        fields: 按发布顺序排列的字段列表。
    """

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
    ) -> None:
        """初始化遥测流结构声明。

        Args:
            stream_id: 流标识。
            fields: 按发布顺序排列的字段列表。
            name: 流名称，默认为空串。
            nominal_rate_hz: 标称发布频率 ``[Hz]``。
        """

class DerivedState:
    """由位置与速度推导出的飞行状态。

    Attributes:
        lat: 纬度 ``[deg]``。
        lon: 经度 ``[deg]``。
        altitude: 海拔高度 ``[m]``。
        alpha: 迎角 ``[rad]``。
        beta: 侧滑角 ``[rad]``。
        tas: 真空速 ``[m/s]``。
        eas: 当量空速 ``[m/s]``。
        gamma: 航迹倾角 ``[rad]``。
        chi: 航迹方位角 ``[rad]``。
        ias: 指示空速 ``[m/s]``，``None`` 表示未提供。
        cas: 校准空速 ``[m/s]``，``None`` 表示未提供。
        mach: 马赫数，``None`` 表示未提供。
        ground_speed: 地速 ``[m/s]``，``None`` 表示未提供。
        vertical_speed: 垂直速度 ``[m/s]``，``None`` 表示未提供。
        dynamic_pressure: 动压 ``[Pa]``，``None`` 表示未提供。
        normal_load_factor: 法向过载，``None`` 表示未提供。
    """

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
    ) -> None:
        """初始化派生状态。

        Args:
            lat: 纬度 ``[deg]``。
            lon: 经度 ``[deg]``。
            altitude: 海拔高度 ``[m]``。
            alpha: 迎角 ``[rad]``，默认 ``0.0``。
            beta: 侧滑角 ``[rad]``，默认 ``0.0``。
            tas: 真空速 ``[m/s]``，默认 ``0.0``。
            eas: 当量空速 ``[m/s]``，默认 ``0.0``。
            gamma: 航迹倾角 ``[rad]``，默认 ``0.0``。
            chi: 航迹方位角 ``[rad]``，默认 ``0.0``。
            ias: 指示空速 ``[m/s]``。
            cas: 校准空速 ``[m/s]``。
            mach: 马赫数。
            ground_speed: 地速 ``[m/s]``。
            vertical_speed: 垂直速度 ``[m/s]``。
            dynamic_pressure: 动压 ``[Pa]``。
            normal_load_factor: 法向过载。
        """
    def __repr__(self) -> str:
        """返回派生状态的可读表示。

        Returns:
            调试用字符串。
        """

class AircraftState:
    """一次状态更新的完整飞机快照。

    Attributes:
        position: NED 位置向量 ``[m]``。
        velocity: 机体系 BODY-FRD 速度向量 ``[m/s]``（x 前、y 右、z 下）。
        attitude: 姿态。
        angular_velocity: 机体系角速度 ``[rad/s]``。
        derived: 可选派生状态。
        control_surfaces: 可选控制面状态。
        linear_acceleration_body: 可选机体系线加速度 ``[m/s^2]``。
        propulsors: 推进器状态列表。
    """

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
    ) -> None:
        """初始化飞机状态，省略的字段使用各自默认值。

        Args:
            position: NED 位置向量 ``[m]``。
            velocity: 机体系 BODY-FRD 速度向量 ``[m/s]``（x 前、y 右、z 下）。
            attitude: 姿态。
            angular_velocity: 机体系角速度 ``[rad/s]``。
            derived: 派生状态。
            control_surfaces: 控制面状态。
            linear_acceleration_body: 机体系线加速度 ``[m/s^2]``。
            propulsors: 推进器状态列表。
        """
    @staticmethod
    def hover() -> "AircraftState":
        """返回悬停配置的飞机状态。

        Returns:
            位置、速度与角速度均为零的默认状态。
        """
    def __repr__(self) -> str:
        """返回飞机状态的可读表示。

        Returns:
            调试用字符串。
        """

class PyClient:
    """Rust 侧的 UDP 协议客户端。

    由 ``fly_ruler_proto_python.client.FlyRulerClient`` 封装后使用。
    """

    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: AircraftState | None = None,
        toml_config: str = "",
        heartbeat_interval_secs: float = 1.0,
        telemetry_schemas: list[TelemetryStreamSchema] | None = None,
        spawn_timestamp: float | None = None,
    ) -> None:
        """连接服务端、完成握手并生成一架飞机。

        Args:
            addr: 服务端 ``host:port`` 地址。
            aircraft_name: 飞机型号名称。
            initial_state: 初始状态，``None`` 时由核心填充默认值。
            toml_config: 飞机 TOML 配置文本，默认空串。
            heartbeat_interval_secs: 心跳周期 ``[s]``，默认 ``1.0``。
            telemetry_schemas: 遥测流声明列表。
            spawn_timestamp: 生成时刻时间戳 ``[s]``，``None`` 表示由核心决定。
        """
    def client_uuid(self) -> str:
        """返回客户端 UUID。

        Returns:
            UUID 字符串。
        """
    def aircraft_uuid(self) -> str:
        """返回已生成飞机的 UUID。

        Returns:
            UUID 字符串。
        """
    def update_state(
        self, state: AircraftState, timestamp: float | None = None
    ) -> None:
        """上报一次飞机状态。

        Args:
            state: 新的飞机状态快照。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。
        """
    def create_event(self, event_name: str, timestamp: float | None = None) -> None:
        """创建一个命名事件。

        Args:
            event_name: 事件名称。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。
        """
    def publish_telemetry(
        self,
        stream_id: str,
        values: tuple[float | int | bool, ...],
        timestamp: float | None = None,
    ) -> None:
        """按流声明的字段顺序发布一帧遥测数据。

        Args:
            stream_id: 目标遥测流标识。
            values: 与流字段顺序一一对应的取值。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。
        """
    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None:
        """销毁当前飞机。

        Args:
            reason: 销毁原因，``None`` 表示不附带原因。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。
        """
    def close(self) -> None:
        """关闭网络连接并释放底层资源。"""
