"""
Fly Ruler Protocol Python Bindings.

A high-performance binary serialization protocol library for aerospace flight simulation.
"""

from enum import IntEnum

from fly_ruler_proto_python._core import (
    PROTOCOL_VERSION,
    AircraftState,
    Attitude,
    ControlSurfaceState,
    DerivedState,
    PropulsorState,
    TelemetryField,
    TelemetryStreamSchema,
    TelemetryValueType,
    Vector3,
    get_protocol_version,
)
from fly_ruler_proto_python.client import FlyRulerClient, create_aircraft_state


class PropulsorKind(IntEnum):
    """Cross-aircraft propulsor category used by :class:`PropulsorState`."""

    UNSPECIFIED = 0
    JET = 1
    PROPELLER = 2
    ROTOR = 3


__all__ = [
    # Version
    "PROTOCOL_VERSION",
    "get_protocol_version",
    # Core types
    "Vector3",
    "Attitude",
    "DerivedState",
    "ControlSurfaceState",
    "PropulsorState",
    "PropulsorKind",
    "TelemetryValueType",
    "TelemetryField",
    "TelemetryStreamSchema",
    "AircraftState",
    # High-level API
    "FlyRulerClient",
    "create_aircraft_state",
]
