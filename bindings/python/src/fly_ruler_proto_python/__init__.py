"""FlyRuler 协议的 Python 绑定。

面向航空航天飞行仿真的高性能二进制序列化协议库。
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
    """``PropulsorState`` 使用的跨机型推进器类别。

    Attributes:
        UNSPECIFIED: 未指定类别，值为 ``0``。
        JET: 喷气推进，值为 ``1``。
        PROPELLER: 螺旋桨推进，值为 ``2``。
        ROTOR: 旋翼推进，值为 ``3``。
    """

    UNSPECIFIED = 0
    JET = 1
    PROPELLER = 2
    ROTOR = 3


__all__ = [
    # 版本
    "PROTOCOL_VERSION",
    "get_protocol_version",
    # 核心类型
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
    # 高层 API
    "FlyRulerClient",
    "create_aircraft_state",
]


def main() -> None:
    """打印协议版本与公开导出面。

    作为 ``fly_ruler_proto_python`` console script 的入口，用于安装后快速
    确认绑定可用，无需连接服务端；只做只读展示，不改动 ``__all__``。
    """
    print(f"fly_ruler_proto_python 协议版本：{PROTOCOL_VERSION}")
    print(f"``get_protocol_version()``：{get_protocol_version()}")
    print(f"公开导出（{len(__all__)} 项）：")
    for name in __all__:
        print(f"  - {name}")
