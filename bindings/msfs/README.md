# fly_ruler_proto_msfs

`fly_ruler_proto_msfs` 是 FlyRuler 到 Microsoft Flight Simulator 2024 的桥接（Rust，Windows 目标，基于 `simconnect`）。它订阅协议内核的快照，把机位姿写到 MSFS 仿真变量，并可选地把额外飞机渲染成 AI 机。

## 目录结构

- `src/main.rs` —— 桥接入口：加载配置并驱动渲染循环。
- `src/bridge.rs` —— 快照消费、平滑与写回 MSFS 的主循环。
- `src/simconnect.rs` —— SimConnect 绑定，只在 Windows 目标编入。
- `src/smoothing.rs` —— 插值与短时外推，`smoothing_mode` 可取 `latest`、`low_latency`（默认）、`smooth`。
- `src/aircraft.rs`、`src/ai.rs` —— 用户机与 AI 机的状态映射。
- `src/config.rs` —— 配置解析；会话数据放在 `sessions/<会话>/` 下的 `data_root`。
- `fly-ruler-msfs.example.toml` —— 配置模板，`[bridge]` 段给出监听地址、`tick_hz`、平滑参数与多机渲染开关。

## 构建与运行

- 交叉构建：`just build-msfs` 与 `just build-msfs-release`（`cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc`）。
- 静态检查：`just check-msfs`。
- 打包：`just package-msfs`，产物是 `dist/fly-ruler-msfs-windows-x86_64.zip`。

这些命令需要 Windows 目标工具链，不属于本机默认的 `just check` / `just test` 范围。`sessions/` 是运行时会话目录，已被 `.gitignore` 忽略，不随源码分发。
