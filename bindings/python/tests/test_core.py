"""面向当前公开面的 Python 绑定单元测试。"""

from __future__ import annotations

from importlib.metadata import version

import pytest

import fly_ruler_proto_python.client as client_module
from fly_ruler_proto_python import (
    PROTOCOL_VERSION,
    AircraftState,
    Attitude,
    ControlSurfaceState,
    DerivedState,
    FlyRulerClient,
    PropulsorKind,
    PropulsorState,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
    Vector3,
    create_aircraft_state,
    get_protocol_version,
)


class TestVector3:
    """向量对象的构造与可变性。"""

    def test_create_and_mutate(self) -> None:
        """构造后三个分量可读，且可以逐字段改写。"""
        v = Vector3(1.0, 2.0, 3.0)
        assert v.x == 1.0
        assert v.y == 2.0
        assert v.z == 3.0

        v.x = 9.0
        v.y = 8.0
        v.z = 7.0
        assert (v.x, v.y, v.z) == (9.0, 8.0, 7.0)

    def test_zero(self) -> None:
        """零向量工厂返回三分量全零对象。"""
        v = Vector3.zero()
        assert (v.x, v.y, v.z) == (0.0, 0.0, 0.0)


class TestAttitude:
    """姿态对象的构造与表示互转。"""

    def test_identity(self) -> None:
        """单位姿态的四元数是 (1, 0, 0, 0)。"""
        attitude = Attitude.identity()
        assert attitude.quaternion == (1.0, 0.0, 0.0, 0.0)

    def test_representations(self) -> None:
        """零欧拉角对应单位旋转矩阵，且能还原回单位四元数。"""
        attitude = Attitude.from_euler((0.0, 0.0, 0.0))
        assert attitude.rotation_matrix == (
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
        )
        assert Attitude.from_rotation_matrix(attitude.rotation_matrix).quaternion == (
            1.0,
            0.0,
            0.0,
            0.0,
        )


class TestDerivedState:
    """派生量的构造与字段读写。"""

    def test_create(self) -> None:
        """按关键字构造后每个分量都保留输入值。"""
        d = DerivedState(
            lat=37.7749,
            lon=-122.4194,
            altitude=500.0,
            alpha=0.05,
            beta=0.0,
            tas=50.0,
            eas=48.0,
            gamma=0.1,
            chi=0.2,
            ias=47.0,
            cas=47.5,
            mach=0.15,
        )
        assert d.lat == 37.7749
        assert d.lon == -122.4194
        assert d.altitude == 500.0
        assert d.alpha == 0.05
        assert d.beta == 0.0
        assert d.tas == 50.0
        assert d.eas == 48.0
        assert d.gamma == 0.1
        assert d.chi == 0.2
        assert d.ias == 47.0
        assert d.cas == 47.5
        assert d.mach == 0.15


class TestAircraftState:
    """飞机状态的构造方式与默认值。"""

    def test_hover(self) -> None:
        """悬停工厂给出零位置、零速度与单位姿态。"""
        state = AircraftState.hover()
        assert state.position.x == 0.0
        assert state.velocity.x == 0.0
        assert state.attitude.quaternion[0] == 1.0

    def test_create_with_derived(self) -> None:
        """显式传入派生量时按值保存。"""
        state = AircraftState(
            position=Vector3(100.0, 200.0, -300.0),
            velocity=Vector3(1.0, 2.0, 3.0),
            attitude=Attitude.identity(),
            angular_velocity=Vector3(0.1, 0.2, 0.3),
            derived=DerivedState(
                lat=30.0,
                lon=120.0,
                altitude=1000.0,
                alpha=0.1,
                beta=0.2,
                tas=250.0,
                eas=240.0,
                gamma=0.0,
                chi=1.0,
            ),
        )
        assert state.position.y == 200.0
        assert state.angular_velocity.z == 0.3
        assert state.derived is not None
        assert state.derived.tas == 250.0

    def test_standard_controls_and_propulsors(self) -> None:
        """控制面与推进器列表按输入顺序保存。"""
        state = AircraftState(
            control_surfaces=ControlSurfaceState(
                elevator_rad=0.1,
                flaps_left_ratio=0.5,
            ),
            propulsors=[
                PropulsorState(
                    "engine.left", kind=PropulsorKind.JET, index=1, throttle_ratio=0.25
                ),
                PropulsorState(
                    "engine.right", kind=PropulsorKind.JET, index=2, throttle_ratio=0.75
                ),
            ],
        )
        surfaces = state.control_surfaces
        assert surfaces is not None
        assert surfaces.elevator_rad == 0.1
        assert surfaces.flaps_left_ratio == 0.5
        assert [propulsor.index for propulsor in state.propulsors] == [1, 2]
        assert state.propulsors[1].throttle_ratio == 0.75

    def test_acceleration_and_propulsor_state(self) -> None:
        """机体系加速度与旋翼推进器的扩展字段可读。"""
        state = create_aircraft_state(
            linear_acceleration_body=(0.1, 0.2, -9.7),
            propulsors=[
                PropulsorState(
                    "left_rotor",
                    kind=PropulsorKind.ROTOR,
                    throttle_ratio=0.6,
                    rpm=2400.0,
                    blade_pitch_rad=0.1,
                    thrust_newton=120.0,
                    torque_newton_meter=15.0,
                    index=1,
                )
            ],
        )
        acceleration = state.linear_acceleration_body
        assert acceleration is not None
        assert acceleration.z == -9.7
        assert state.propulsors[0].kind == PropulsorKind.ROTOR
        assert state.propulsors[0].rpm == 2400.0
        assert state.propulsors[0].index == 1
        assert state.propulsors[0].thrust_newton == 120.0


class TestHelpers:
    """高层构造助手的默认值与转发行为。"""

    def test_create_aircraft_state_helper(self) -> None:
        """助手接受的元组与对象参数都落到状态对象上。"""
        state = create_aircraft_state(
            position=(1.0, 2.0, 3.0),
            velocity=(4.0, 5.0, 6.0),
            attitude=Attitude.from_quaternion((1.0, 0.1, 0.2, 0.3)),
            angular_velocity=(0.4, 0.5, 0.6),
            derived=DerivedState(31.2, 121.5, 1000.0),
            control_surfaces=ControlSurfaceState(rudder_rad=0.1),
            propulsors=[
                PropulsorState(
                    "engine", kind=PropulsorKind.JET, index=1, throttle_ratio=0.4
                )
            ],
        )
        surfaces = state.control_surfaces
        assert state.position.x == 1.0
        assert state.velocity.y == 5.0
        assert state.attitude.quaternion[3] > 0.0
        assert state.angular_velocity.x == 0.4
        assert state.derived is not None
        assert surfaces is not None
        assert surfaces.rudder_rad == 0.1
        assert state.propulsors[0].throttle_ratio == 0.4


class TestModuleApi:
    """模块级常量与函数的公开面。"""

    def test_protocol_version(self) -> None:
        """协议版本与已安装包版本一致，函数形式返回同一值。"""
        assert PROTOCOL_VERSION == version("fly_ruler_proto_python")
        assert get_protocol_version() == PROTOCOL_VERSION


class _FakePyClient:
    """用于包装层行为测试的内存替身。"""

    instances: list[_FakePyClient] = []

    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: AircraftState,
        toml_config: str,
        heartbeat_interval_secs: float,
        telemetry_schemas: list[TelemetryStreamSchema],
        spawn_timestamp: float | None,
    ) -> None:
        self.addr = addr
        self.aircraft_name = aircraft_name
        self.initial_state = initial_state
        self.toml_config = toml_config
        self.heartbeat_interval_secs = heartbeat_interval_secs
        self.telemetry_schemas = telemetry_schemas
        self.spawn_timestamp = spawn_timestamp
        self.closed = False
        self.calls: list[tuple[object, ...]] = []
        _FakePyClient.instances.append(self)

    def client_uuid(self) -> str:
        """返回固定的客户端标识。"""
        return "fake-client-uuid"

    def aircraft_uuid(self) -> str:
        """返回固定的飞机标识。"""
        return "fake-aircraft-uuid"

    def update_state(
        self, state: AircraftState, timestamp: float | None = None
    ) -> None:
        """记录一次状态更新调用。"""
        self.calls.append(("update_state", state, timestamp))

    def create_event(self, event_name: str, timestamp: float | None = None) -> None:
        """记录一次事件上报调用。"""
        self.calls.append(("create_event", event_name, timestamp))

    def publish_telemetry(
        self,
        stream_id: str,
        values: tuple[float | int | bool, ...],
        timestamp: float | None = None,
    ) -> None:
        """记录一次遥测发布调用。"""
        self.calls.append(("publish_telemetry", stream_id, values, timestamp))

    def despawn(
        self, reason: str | None = None, timestamp: float | None = None
    ) -> None:
        """记录一次注销调用。"""
        self.calls.append(("despawn", reason, timestamp))

    def close(self) -> None:
        """记录关闭调用并标记为已关闭。"""
        self.closed = True
        self.calls.append(("close",))


class TestFlyRulerClient:
    """高层客户端对底层扩展对象的转发与校验。"""

    def test_wrapper_forwards_calls(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """包装层把参数原样转给底层对象，并暴露两个标识属性。"""
        _FakePyClient.instances.clear()
        monkeypatch.setattr(client_module, "PyClient", _FakePyClient)

        schema = TelemetryStreamSchema(
            "controller",
            [
                TelemetryField(
                    "controller.pitch.error", value_type=TelemetryValueType.F64
                ),
                TelemetryField(
                    "controller.saturated", value_type=TelemetryValueType.Bool
                ),
            ],
            nominal_rate_hz=100.0,
        )
        client = FlyRulerClient(
            "127.0.0.1:9000",
            "F-16",
            toml_config="[aircraft]\nname='F-16'",
            heartbeat_interval_secs=0.5,
            telemetry_schemas=[schema],
            spawn_timestamp=0.0,
        )

        inner = _FakePyClient.instances[-1]
        assert inner.addr == "127.0.0.1:9000"
        assert inner.aircraft_name == "F-16"
        assert inner.spawn_timestamp == 0.0
        assert client.client_uuid == "fake-client-uuid"
        assert client.aircraft_uuid == "fake-aircraft-uuid"

        state = create_aircraft_state(position=(10.0, 20.0, -30.0))
        client.update_state(state, timestamp=123.0)
        client.create_event("missile_launch", timestamp=124.0)
        client.publish_telemetry("controller", (0.25, True), timestamp=124.5)
        client.despawn(reason="done", timestamp=125.0)
        client.close()

        assert inner.calls[0][0] == "update_state"
        assert inner.calls[1] == ("create_event", "missile_launch", 124.0)
        assert inner.calls[2] == (
            "publish_telemetry",
            "controller",
            (0.25, True),
            124.5,
        )
        assert inner.calls[3] == ("despawn", "done", 125.0)
        assert inner.calls[4] == ("close",)

    def test_context_manager_closes_once(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """退出 with 块只关闭一次底层对象。"""
        _FakePyClient.instances.clear()
        monkeypatch.setattr(client_module, "PyClient", _FakePyClient)

        with FlyRulerClient("127.0.0.1:9001", "J-20") as client:
            assert client.aircraft_uuid == "fake-aircraft-uuid"

        inner = _FakePyClient.instances[-1]
        close_calls = [c for c in inner.calls if c[0] == "close"]
        assert len(close_calls) == 1

    def test_rejects_non_finite_source_timestamps(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """非有限时间戳在发起调用前就被拒绝。"""
        _FakePyClient.instances.clear()
        monkeypatch.setattr(client_module, "PyClient", _FakePyClient)

        with pytest.raises(ValueError, match="spawn_timestamp must be finite"):
            FlyRulerClient("127.0.0.1:9001", "quad", spawn_timestamp=float("nan"))

        client = FlyRulerClient("127.0.0.1:9001", "quad", spawn_timestamp=0.0)
        state = create_aircraft_state()
        with pytest.raises(ValueError, match="timestamp must be finite"):
            client.update_state(state, timestamp=float("inf"))
        with pytest.raises(ValueError, match="timestamp must be finite"):
            client.create_event("event", timestamp=float("nan"))
        with pytest.raises(ValueError, match="timestamp must be finite"):
            client.publish_telemetry("stream", (), timestamp=float("nan"))
        with pytest.raises(ValueError, match="timestamp must be finite"):
            client.despawn(timestamp=float("nan"))
        client.close()
