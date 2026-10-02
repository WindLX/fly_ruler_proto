"""基于 Rust ``PyClient`` 的面向飞机生命周期的 Python 封装。"""

from __future__ import annotations

import math
from collections.abc import Sequence
from types import TracebackType

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
    """校验可选时间戳是否为有限值。

    Args:
        name: 出错信息中使用的参数名。
        timestamp: 待校验的时间戳 ``[s]``，``None`` 表示跳过校验。

    Raises:
        ValueError: 时间戳不是有限值。
    """
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
    """创建带默认值的 ``AircraftState``，便于脚本化构造。

    Args:
        position: NED 位置 ``(x, y, z)``，单位 ``[m]``。
        velocity: 机体系 BODY-FRD 速度 ``(vx, vy, vz)``，单位 ``[m/s]``（x 前、y 右、z 下）。
        attitude: 姿态；``None`` 时使用 ``Attitude.identity()``。
        angular_velocity: 机体系角速度 ``(p, q, r)``，单位 ``[rad/s]``。
        derived: 可选的派生状态。
        control_surfaces: 可选的控制面状态。
        linear_acceleration_body: 可选的机体系线加速度 ``[m/s^2]``。
        propulsors: 可选的推进器状态列表。

    Returns:
        构造完成的 ``AircraftState`` 实例。
    """
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
    """绑定单个飞机生命周期的客户端。

    构造函数内自动连接、握手并生成一架飞机；``close()`` 或上下文退出时自动
    销毁飞机并关闭网络连接。

    Attributes:
        client_uuid: 客户端 UUID 字符串。
        aircraft_uuid: 已生成飞机的 UUID 字符串。

    Note:
        连接、握手与生成飞机都在构造函数内完成，失败时直接抛出异常，不会返回
        半初始化的实例。

    Examples:
        ```python
        with FlyRulerClient("127.0.0.1:18002", "F-16") as aircraft:
            state = create_aircraft_state(position=(100.0, 0.0, -1000.0))
            aircraft.update_state(state)
            aircraft.create_event("missile_launch")
        ```
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
        """初始化客户端并生成一架飞机。

        Args:
            address: 服务端 ``host:port`` 地址。
            aircraft_name: 飞机型号名称。
            initial_state: 初始状态，``None`` 时使用默认状态的副本。
            toml_config: 飞机 TOML 配置文本，默认空串。
            heartbeat_interval_secs: 心跳周期 ``[s]``，默认 ``1.0``。
            telemetry_schemas: 遥测流声明序列，默认空。
            spawn_timestamp: 生成时刻时间戳 ``[s]``，``None`` 表示由核心决定。

        Raises:
            ValueError: 心跳周期不是有限正数，或生成时间戳不是有限值。
        """
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
        """返回客户端 UUID。

        Returns:
            UUID 字符串。
        """
        return self._inner.client_uuid()

    @property
    def aircraft_uuid(self) -> str:
        """返回已生成飞机的 UUID。

        Returns:
            UUID 字符串。
        """
        return self._inner.aircraft_uuid()

    def update_state(
        self,
        state: AircraftState,
        timestamp: float | None = None,
    ) -> None:
        """上报一次飞机状态。

        Args:
            state: 新的飞机状态快照。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。

        Raises:
            ValueError: 时间戳不是有限值。
        """
        _validate_timestamp("timestamp", timestamp)
        self._inner.update_state(state, timestamp)

    def create_event(
        self,
        event_name: str,
        timestamp: float | None = None,
    ) -> None:
        """创建一个命名事件。

        Args:
            event_name: 事件名称。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。

        Raises:
            ValueError: 时间戳不是有限值。
        """
        _validate_timestamp("timestamp", timestamp)
        self._inner.create_event(event_name, timestamp)

    def publish_telemetry(
        self,
        stream_id: str,
        values: Sequence[float | int | bool],
        timestamp: float | None = None,
    ) -> None:
        """按流声明的字段顺序发布一帧遥测数据。

        Args:
            stream_id: 目标遥测流标识。
            values: 与流字段顺序一一对应的取值。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。

        Raises:
            ValueError: 时间戳不是有限值。
        """
        _validate_timestamp("timestamp", timestamp)
        self._inner.publish_telemetry(stream_id, tuple(values), timestamp)

    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None:
        """销毁当前客户端持有的飞机。

        Args:
            reason: 销毁原因，``None`` 表示不附带原因。
            timestamp: 时间戳 ``[s]``，``None`` 表示由核心决定。

        Raises:
            ValueError: 时间戳不是有限值。
        """
        _validate_timestamp("timestamp", timestamp)
        self._inner.despawn(reason, timestamp)

    def close(self) -> None:
        """关闭网络连接，重复调用不会产生额外效果。"""
        if not self._closed:
            self._inner.close()
            self._closed = True

    def __enter__(self) -> "FlyRulerClient":
        """进入上下文并返回自身。

        Returns:
            当前客户端实例。
        """
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """退出上下文并关闭连接。

        Args:
            exc_type: 异常类型；正常退出时为 ``None``。
            exc_val: 异常实例；正常退出时为 ``None``。
            exc_tb: 异常回溯对象；正常退出时为 ``None``。
        """
        self.close()

    def __del__(self) -> None:
        """对象回收时尽力关闭连接，忽略关闭过程中的异常。"""
        try:
            self.close()
        except Exception:
            pass
