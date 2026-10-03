# 排障

## 握手被拒或版本不匹配

现象是客户端或 MSFS 桥在握手阶段收到 `ProtocolVersionMismatch`，错误文本为 `protocol version mismatch`，连接随即失败。

服务端把握手包里的 `version` 与编译期的 `PROTOCOL_VERSION` 做比较（`core/src/transport/server.rs:446`），不一致时用 `ErrorCode::ProtocolVersionMismatch` 回该文本（`core/src/transport/server.rs:460-466`）。

处理办法是让两端来自同一版本：Python 客户端与 `fly-ruler-server`、MSFS 桥与内核的版本必须配对，升级时一起升，不要只换其中一边。

## 桥一直停在等待 SimConnect

日志里反复出现 `waiting for MSFS 2024 SimConnect`，并且每秒重试一次（`bindings/msfs/src/bridge.rs:63-71`）。

这说明桥进程本身已经起来，但连不上 SimConnect，通常是因为模拟器还没进到可飞状态。

确认 MSFS 2024 已经加载完成并进入 Free Flight；停在主菜单或还在载入时，桥会一直等下去，这是预期行为。

## MSFS 里飞机不动

先确认客户端连的是桥监听的 UDP 端口，默认 `127.0.0.1:18002`（`bindings/msfs/src/config.rs:150-153`），没有数据到达时桥不会做任何事。

再确认状态时间戳在推进：桥按 `1/render_hz` 取样（`bindings/msfs/src/bridge.rs:48`），超过 `stale_timeout_ms` 没有新状态就会停在上一次的位置，默认超时是 500 毫秒（`bindings/msfs/src/config.rs:184-187`）。

如果数据在推进但画面抖动或滞后，调整平滑参数：`smoothing_mode` 默认 `low_latency`，可以改成 `latest` 或 `smooth`，并相应设置 `interpolation_delay_ms` 与 `max_extrapolation_ms`（`bindings/msfs/src/config.rs:155-177`，取值与默认见 `bindings/msfs/src/smoothing.rs:10-19` 与 `:62-66`）。

采样频率不要低于客户端上报频率，否则看起来同样像卡住；把 `render_hz` 提高到接近 `tick_hz` 可以减少丢样。

## AI 机不出现

AI 机默认关闭，启动桥时加 `--enable-ai-aircraft` 才会启用（`bindings/msfs/src/config.rs:188-192`）。

机型由 `--ai-aircraft-title` 指定，默认 `Rafale M`；该项为空时会报 `ai_aircraft_title must not be empty when AI aircraft are enabled`（`bindings/msfs/src/config.rs:193-196` 与 `:262-263`）。

数量由 `max_ai_aircraft` 控制，默认 8，超过 64 会报 `max_ai_aircraft must be at most 64`（`bindings/msfs/src/config.rs:197-200` 与 `:265-267`）。

Linux + Proton 下机型和涂装包要放进模拟器前缀里的用户数据目录；MSFS 2024 的 Steam AppID 是 2537590（`bindings/msfs/README.md:27`），对应的前缀内路径是 `~/.local/share/Steam/steamapps/compatdata/2537590/pfx/drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024/Packages/Community`，把机型包放进 `Community` 才会被 `--ai-aircraft-title` 匹配到。

## 端口被占用

桥默认监听 UDP `127.0.0.1:18002`（`bindings/msfs/src/config.rs:150-153`），管理面默认 `127.0.0.1:18003`（`bindings/msfs/src/config.rs:201-211`）。

独立服务端的默认值相同：UDP `127.0.0.1:18002`（`server/src/config.rs:143-146`），管理面 `127.0.0.1:18003`（`server/src/config.rs:154-157`）。

同一时间只跑一个占用这些端口的进程；需要并行时改桥配置里的 `listen` 和管理面 `listen`，或给独立服务端传 `--udp-listen` 与 `--http-listen`。

先看清是谁占着：`ss -lntup | grep -E '18002|18003'`；如果之前用 `systemctl --user start fly-ruler-msfs` 起过桥，`systemctl --user status fly-ruler-msfs` 能看出它是不是还在跑。

## Linux + Proton 下启动失败

桥的 Windows 产物必须在 Proton 下运行，直接在 Linux 上执行会打印 `fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton` 并以退出码 2 结束（`bindings/msfs/src/main.rs:10-24`）。

确认拿到的是 `x86_64-pc-windows-msvc` 目标的 exe，再通过 `protontricks-launch --appid 2537590` 一类方式在 MSFS 的 Proton 前缀里启动。

protontricks 提示缺少 `winetricks` 只是警告，桥本身的运行不依赖它。

## 安装脚本装出来的桥

提示 `fly-ruler-msfs: command not found` 时，`~/.local/bin` 不在 `PATH` 里：把 `export PATH="$HOME/.local/bin:$PATH"` 写进 shell 配置，再开一个新终端。

启动命令里记的是安装那一刻 `protontricks-launch` 的绝对路径（`scripts/install-msfs.sh`）；先装 protontricks 再装桥，或者装完 protontricks 后重跑一次安装脚本。

配置在 `~/.config/fly-ruler-msfs/fly-ruler-msfs.toml`，里面的路径都是绝对路径；改完重启桥即生效，重跑安装脚本不会覆盖这份文件。

用 `systemctl --user start fly-ruler-msfs` 启动时日志进 journal，跟日志用 `journalctl --user -u fly-ruler-msfs -f`，停止用 `systemctl --user stop fly-ruler-msfs`。这个 unit 故意没有 `[Install]` 段，`systemctl --user enable` 会失败：桥不是常驻服务，只在需要连模拟器时临时启动。

升级就是再跑一次安装脚本：新版本装进 `~/.local/share/fly-ruler-msfs/versions/`，`current` 软链原子指向它，旧版本仍在原地；想回退就把 `current` 指回旧目录。

从本地源码树装出来的版本目录名带 `local-` 前缀，例如 `versions/local-0.4.0/`；它和 Release 版本并存，`current` 指到最后装的那一个，所以用 `--source` 装了本地修改之后，想回到发布版本再跑一次普通安装即可。

卸载用 `curl -fsSL https://raw.githubusercontent.com/WindLX/fly_ruler_proto/main/scripts/install-msfs.sh | bash -s -- --uninstall`，默认保留配置、日志与会话数据，加 `--purge` 连这些一起删。

## 控制台打不开或空白

管理面只在 `web_root` 下存在 `index.html` 时托管前端，缺文件时直接不提供页面（`core/src/management/server.rs:401-417`）。

`index.html` 存在但没有运行配置标记时会报 `{path} does not contain the runtime configuration placeholder`（`core/src/management/server.rs:401-417`），此时应重新构建前端产物。

`web_root` 默认是 `web/dist`（`server/src/config.rs:164-169`，桥侧见 `bindings/msfs/src/config.rs:212-223`），确认它指向包含 `index.html` 的目录。

调试界面时改用开发模式，Vite 开发服务器监听 5173，配合允许的 CORS 来源即可打开。

## 看日志

`RUST_LOG` 环境变量覆盖配置里的 `logging.level`（`core/src/logging.rs:66-70`），未设置时读取配置。

配置默认级别是 `info`（`server/src/config.rs:207-211`，桥侧见 `bindings/msfs/src/config.rs:238-242`，常量见 `core/src/logging.rs:13`）。

常用写法是 `RUST_LOG=debug` 打开全量调试日志，`RUST_LOG=fly_ruler_proto_core=debug` 只看内核，`RUST_LOG=fly_ruler_proto_server=info` 只看服务端。

日志默认写到标准错误，需要落盘时在配置里设置 `logging.file_path`；安装脚本生成的配置里已经留了一行注释好的示例，取消注释即可写到 `~/.local/state/fly-ruler-msfs/bridge.log`。用 unit 启动时日志还会同时进 journal，`journalctl --user -u fly-ruler-msfs -f` 就能跟。

## 相关页面

- [安装](01-install.md)
- [快速开始](02-quickstart.md)
- [控制台](03-console.md)
