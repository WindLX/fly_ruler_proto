# fly_ruler_proto_python

`fly_ruler_proto_python` 是 FlyRuler 协议的 Python 绑定。Rust 侧 `_core`（PyO3）提供协议类型与 UDP 客户端，Python 侧提供 `FlyRulerClient` 等高层封装，并附带一个命令行自检入口。要求 Python 3.12+。

## 目录结构

- `src/lib.rs`、`src/client.rs`、`src/protocol.rs` —— PyO3 扩展 `fly_ruler_proto_python._core`（`crate-type = cdylib`）。
- `src/fly_ruler_proto_python/` —— 纯 Python 包：`__init__.py`（导出面与 `main()`）、`client.py`（`FlyRulerClient`、`create_aircraft_state`）、`_core.pyi`（类型存根）。
- `examples/` —— UDP 客户端与 MSFS 演示脚本。
- `tests/` —— pytest 回归。

## 安装与运行

- 同步依赖：`uv sync --all-groups`。
- 本地安装扩展：`uv run maturin develop`。
- 自检：`uv run fly_ruler_proto_python`，打印协议版本与公开导出面。

## 测试

- `uv run pytest tests/`（需先构建扩展）。
- 仓库入口：`just check-python`、`just test-python`。
