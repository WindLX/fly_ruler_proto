"""
Fly Ruler Protocol Python Bindings.

A high-performance binary serialization protocol library for aerospace flight simulation.
"""

from fly_ruler_proto_python._core import (
    PROTOCOL_VERSION,
    get_protocol_version,
    Vector3,
    Quaternion,
    DerivedState,
    AircraftState,
)

from fly_ruler_proto_python.client import FlyRulerClient, create_aircraft_state

__all__ = [
    # Version
    "PROTOCOL_VERSION",
    "get_protocol_version",
    # Core types
    "Vector3",
    "Quaternion",
    "DerivedState",
    "AircraftState",
    # High-level API
    "FlyRulerClient",
    "create_aircraft_state",
]
