# fly_ruler_proto_godot

`fly_ruler_proto_godot` 是 FlyRuler 协议的 Godot 4 GDExtension（Rust `godot` crate，feature `custom-godot`）。它把内核的运行时配置、快照与回放游标暴露为 Godot 可调用的 `RefCounted` / `Node` 类型。

## 目录结构

- `src/lib.rs` —— 扩展入口与 GDExtension 类型：`FlyRulerAircraftProfileConfig`、`FlyRulerRuntimeConfig`、`FlyRulerAircraftSnapshot`、`FlyRulerFrameSnapshot`、`FlyRulerRuntime`（`Node`）。
- `src/model.rs`、`src/aircraft_profile.rs` —— 快照帧构造与机型配置文件解析。
- `templates/fly_ruler_proto_godot.gdextension` —— 插件描述（Godot 4.7+，仅 Linux x86_64）。
- `templates/fly_ruler_runtime_example.gd` —— 最小运行时示例。
- `scripts/install_addon.sh` —— 构建 `.so` 与 Web 资产并安装到目标 Godot 工程。

## 安装与运行

- 构建：`cargo build -p fly_ruler_proto_godot`，发布版加 `--release`。
- 安装到工程：`bindings/godot/scripts/install_addon.sh <godot_project> [debug|release]`，依赖 `GODOT4_BIN`（默认 `/usr/bin/godot-mono`）。
