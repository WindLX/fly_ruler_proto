"""Unit tests for Fly Ruler Protocol Python Bindings (current API)."""

from __future__ import annotations

import fly_ruler_proto_python.client as client_module
from fly_ruler_proto_python import (
    AircraftState,
    ControlSurfaceState,
    DerivedState,
    EngineState,
    FlyRulerClient,
    PROTOCOL_VERSION,
    Quaternion,
    Vector3,
    create_aircraft_state,
    get_protocol_version,
)


class TestVector3:
    def test_create_and_mutate(self):
        v = Vector3(1.0, 2.0, 3.0)
        assert v.x == 1.0
        assert v.y == 2.0
        assert v.z == 3.0

        v.x = 9.0
        v.y = 8.0
        v.z = 7.0
        assert (v.x, v.y, v.z) == (9.0, 8.0, 7.0)

    def test_zero(self):
        v = Vector3.zero()
        assert (v.x, v.y, v.z) == (0.0, 0.0, 0.0)


class TestQuaternion:
    def test_create(self):
        q = Quaternion(1.0, 0.0, 0.0, 0.0)
        assert (q.w, q.x, q.y, q.z) == (1.0, 0.0, 0.0, 0.0)

    def test_identity(self):
        q = Quaternion.identity()
        assert (q.w, q.x, q.y, q.z) == (1.0, 0.0, 0.0, 0.0)


class TestDerivedState:
    def test_create(self):
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
    def test_hover(self):
        state = AircraftState.hover()
        assert state.position.x == 0.0
        assert state.velocity.x == 0.0
        assert state.attitude.w == 1.0

    def test_create_with_derived(self):
        state = AircraftState(
            position=Vector3(100.0, 200.0, -300.0),
            velocity=Vector3(1.0, 2.0, 3.0),
            attitude=Quaternion(1.0, 0.0, 0.0, 0.0),
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

    def test_custom_fields_round_trip_supported_types(self):
        state = AircraftState(
            custom_fields={
                "float": 1.5,
                "int": 3,
                "bool": True,
                "string": "ok",
                "bytes": b"data",
            }
        )
        assert state.custom_fields == {
            "float": 1.5,
            "int": 3,
            "bool": True,
            "string": "ok",
            "bytes": b"data",
        }
        state.set_custom_field("float", 2.5)
        assert state.custom_fields["float"] == 2.5

    def test_standard_controls_and_engines(self):
        state = AircraftState(
            control_surfaces=ControlSurfaceState(
                elevator_rad=0.1,
                flaps_left_ratio=0.5,
            ),
            engines=[
                EngineState(1, throttle_lever_ratio=0.25),
                EngineState(2, throttle_lever_ratio=0.75),
            ],
        )
        assert state.control_surfaces.elevator_rad == 0.1
        assert state.control_surfaces.flaps_left_ratio == 0.5
        assert [engine.index for engine in state.engines] == [1, 2]
        assert state.engines[1].throttle_lever_ratio == 0.75


class TestHelpers:
    def test_create_aircraft_state_helper(self):
        state = create_aircraft_state(
            position=(1.0, 2.0, 3.0),
            velocity=(4.0, 5.0, 6.0),
            attitude=(1.0, 0.1, 0.2, 0.3),
            angular_velocity=(0.4, 0.5, 0.6),
            derived=DerivedState(31.2, 121.5, 1000.0),
            control_surfaces=ControlSurfaceState(rudder_rad=0.1),
            engines=[EngineState(1, 0.4)],
            custom_fields={"flyruler.control.rudder_rad": 0.1},
        )
        assert state.position.x == 1.0
        assert state.velocity.y == 5.0
        assert state.attitude.z == 0.3
        assert state.angular_velocity.x == 0.4
        assert state.derived is not None
        assert state.control_surfaces.rudder_rad == 0.1
        assert state.engines[0].throttle_lever_ratio == 0.4
        assert state.custom_fields["flyruler.control.rudder_rad"] == 0.1


class TestModuleApi:
    def test_protocol_version(self):
        assert PROTOCOL_VERSION == "1.0.0"
        assert get_protocol_version() == PROTOCOL_VERSION


class _FakePyClient:
    """In-memory test double for wrapper behavior tests."""

    instances = []

    def __init__(
        self,
        addr: str,
        aircraft_name: str,
        initial_state: AircraftState,
        toml_config: str,
        heartbeat_interval_secs: float,
    ) -> None:
        self.addr = addr
        self.aircraft_name = aircraft_name
        self.initial_state = initial_state
        self.toml_config = toml_config
        self.heartbeat_interval_secs = heartbeat_interval_secs
        self.closed = False
        self.calls: list[tuple] = []
        _FakePyClient.instances.append(self)

    def client_uuid(self) -> str:
        return "fake-client-uuid"

    def aircraft_uuid(self) -> str:
        return "fake-aircraft-uuid"

    def update_state(self, state: AircraftState, timestamp: float | None = None) -> None:
        self.calls.append(("update_state", state, timestamp))

    def create_event(self, event_name: str, timestamp: float | None = None) -> None:
        self.calls.append(("create_event", event_name, timestamp))

    def despawn(self, reason: str | None = None, timestamp: float | None = None) -> None:
        self.calls.append(("despawn", reason, timestamp))

    def close(self) -> None:
        self.closed = True
        self.calls.append(("close",))


class TestFlyRulerClient:
    def test_wrapper_forwards_calls(self, monkeypatch):
        _FakePyClient.instances.clear()
        monkeypatch.setattr(client_module, "PyClient", _FakePyClient)

        client = FlyRulerClient(
            "127.0.0.1:9000",
            "F-16",
            toml_config="[aircraft]\nname='F-16'",
            heartbeat_interval_secs=0.5,
        )

        inner = _FakePyClient.instances[-1]
        assert inner.addr == "127.0.0.1:9000"
        assert inner.aircraft_name == "F-16"
        assert client.client_uuid == "fake-client-uuid"
        assert client.aircraft_uuid == "fake-aircraft-uuid"

        state = create_aircraft_state(position=(10.0, 20.0, -30.0))
        client.update_state(state, timestamp=123.0)
        client.create_event("missile_launch", timestamp=124.0)
        client.despawn(reason="done", timestamp=125.0)
        client.close()

        assert inner.calls[0][0] == "update_state"
        assert inner.calls[1] == ("create_event", "missile_launch", 124.0)
        assert inner.calls[2] == ("despawn", "done", 125.0)
        assert inner.calls[3] == ("close",)

    def test_context_manager_closes_once(self, monkeypatch):
        _FakePyClient.instances.clear()
        monkeypatch.setattr(client_module, "PyClient", _FakePyClient)

        with FlyRulerClient("127.0.0.1:9001", "J-20") as client:
            assert client.aircraft_uuid == "fake-aircraft-uuid"

        inner = _FakePyClient.instances[-1]
        close_calls = [c for c in inner.calls if c[0] == "close"]
        assert len(close_calls) == 1
