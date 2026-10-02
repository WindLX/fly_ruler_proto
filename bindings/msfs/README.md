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

## 构建

- 交叉构建：`just msfs build` 与 `just msfs build-release`（`cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc`）。
- 静态检查：`just msfs check`。
- 打包：`just msfs package`，产物是 `dist/fly-ruler-msfs-windows-x86_64.zip`。

这些命令需要 Windows 目标工具链，不属于本机默认的 `just check` / `just test` 范围。`sessions/` 是运行时会话目录，已被 `.gitignore` 忽略，不随源码分发。

## 安装

Linux 上不必手工解压：发布脚本把桥装进用户空间，并配好目录、配置与启动命令。

```bash
curl -fsSL https://raw.githubusercontent.com/WindLX/fly_ruler_proto/main/scripts/install-msfs.sh | bash
```

装完程序在 `~/.local/share/fly-ruler-msfs/versions/<版本>/`（`current` 软链指向它），配置在 `~/.config/fly-ruler-msfs/fly-ruler-msfs.toml`（路径都是绝对路径），命令是 `~/.local/bin/fly-ruler-msfs`，工作目录是 `~/.local/state/fly-ruler-msfs`，会话数据在 `~/.local/share/fly-ruler-msfs/sessions/`。加 `--with-service` 会额外写一个 systemd user unit，只写文件、不 enable 也不启动；卸载用 `--uninstall`，连配置与会话数据一起删就再加 `--purge`。

脚本本体在 `scripts/install-msfs.sh`。开发时用仓库里的 `bindings/msfs/fly-ruler-msfs.dev.toml`（相对路径、相对仓库根），`just msfs run` 默认带这份配置，可用 `FR_MSFS_CONFIG` 换成别的文件。

## 运行

启动前先在 MSFS 2024 里进入 Free Flight 并关闭 Active Pause，否则桥接会一直打印 `waiting for MSFS 2024 SimConnect` 并每秒重试一次（`bindings/msfs/src/bridge.rs:64-70`）。

Windows 上直接运行 `fly-ruler-msfs-bridge.exe`；Linux 上让产物跑在 MSFS 的 Proton 前缀里，用 `protontricks-launch --appid 2537590 ./fly-ruler-msfs-bridge.exe`（MSFS 2024 的 Steam AppID 是 2537590）。

在启动目录放一份 `fly-ruler-msfs.toml` 就会被自动加载，该文件不存在时只用命令行与内置默认值，也可以用 `--config` 指向其它路径（`bindings/msfs/src/config.rs:9`、`bindings/msfs/src/config.rs:132-135`）；安装脚本写的启动命令就是显式传 `--config ~/.config/fly-ruler-msfs/fly-ruler-msfs.toml`，所以从任何工作目录启动都一样。默认日志级别是 `info`（`bindings/msfs/src/config.rs:242`）。

默认 UDP 监听是 `127.0.0.1:18002`（`bindings/msfs/src/config.rs:153`），管理面默认 `127.0.0.1:18003`（`bindings/msfs/src/config.rs:211`）；启动目录下存在 `web/dist` 时管理面会直接托管控制台（`bindings/msfs/src/config.rs:221`）。

要把其余飞机渲染成 AI 机，启动时加 `--enable-ai-aircraft`（`bindings/msfs/src/config.rs:35-36`），数量上限由 `--max-ai-aircraft` 控制，默认 8（`bindings/msfs/src/config.rs:200`）。

非 Windows 目标编译出来的产物不会尝试连接模拟器，而是打印 `fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton` 并以状态码 2 退出（`bindings/msfs/src/main.rs:10-24`）。
