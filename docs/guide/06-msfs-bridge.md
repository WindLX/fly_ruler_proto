# MSFS 2024 桥接

`fly-ruler-msfs-bridge` 把 FlyRuler 收到的飞机状态写进 Microsoft Flight Simulator 2024：它自身就是协议服务端，用 UDP 接收客户端上报的状态样本，按渲染周期经平滑后映射成 MSFS 仿真变量写回模拟器。下面依次说明前提、构建与运行、字段映射，以及与控制台配合和常见失败原因。

## 前提

- 目标平台是 Windows，或在 Linux 上通过 Proton 运行 Steam 版 MSFS 2024（appid 2537590）。
- MSFS 2024 已安装，并且至少进入过一次 Free Flight，否则 SimConnect 无法建立会话。
- 本机需要 MSFS 2024 SDK 的 SimConnect 头文件与导入库，构建脚本要求存在 `SimConnect SDK/include/SimConnect.h`、`SimConnect SDK/lib/SimConnect.lib` 与 `SimConnect.dll`（`bindings/msfs/build.rs`）。SDK 根目录取环境变量 `MSFS2024_SDK`，未设置时回落到仓库根的 `.msfs2024-sdk/MSFS 2024 SDK`（`bindings/msfs/build.rs`）。
- 在 Linux 上交叉编译需要 `cargo-xwin` 与 `rustup target add x86_64-pc-windows-msvc`；运行需要 `protontricks-launch`。构建脚本会链接 `SimConnect` 并把 `SimConnect.dll` 复制到目标输出目录。

## 构建与运行

```bash
just msfs build             # 交叉编译 Windows debug
just msfs build-release     # 交叉编译 Windows release
just msfs check             # Windows 目标的 clippy
just msfs package           # 打包 dist/fly-ruler-msfs-windows-x86_64.zip
just msfs run               # 在 Proton 前缀里运行 debug 产物
```

这些任务由 `just msfs` 分派到私有 recipe（`justfile:48-63`），实际执行 `cargo xwin build -p fly_ruler_proto_msfs --target x86_64-pc-windows-msvc`（`justfile:145-149`）。`just msfs run` 通过 `protontricks-launch --appid 2537590` 启动 `target/x86_64-pc-windows-msvc/debug/fly-ruler-msfs-bridge.exe`（`justfile:155-156`）；打包命令产出包含可执行文件、`SimConnect.dll`、示例配置与控制台产物的压缩包。

非 Windows 目标编译出的程序不会尝试连模拟器，而是打印 `fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton` 并以状态码 2 退出（`bindings/msfs/src/main.rs:15-26`）。真机运行前需要先启动 MSFS 并进入 Free Flight，同时关闭 Active Pause。

桥接启动时先绑定 UDP 监听地址，再按 `[management]` 决定是否起管理服务（`bindings/msfs/src/bridge.rs:32-45`）。连接 SimConnect 失败不会退出，而是每秒重试并记录 `waiting for MSFS 2024 SimConnect`（`bindings/msfs/src/bridge.rs:64-70`）；连上后再等一个有效的 FlyRuler 飞机状态（`bindings/msfs/src/bridge.rs:72`）。

## 配置

默认配置文件名是 `fly-ruler-msfs.toml`，且只有启动目录下确实存在该文件时才会自动加载（`bindings/msfs/src/config.rs:9`、`bindings/msfs/src/config.rs:132-135`）。示例 `fly-ruler-msfs.example.toml` 的主要项与默认值如下。

| 配置项 | 默认值 | 说明 |
| --- | --- | --- |
| `bridge.listen` | `"127.0.0.1:18002"` | UDP 接收地址（`bindings/msfs/src/config.rs:153`） |
| `bridge.tick_hz` | `240.0` | `render_hz` 缺省时的取值（`bindings/msfs/src/config.rs:155-156`） |
| `bridge.render_hz` | 等于 `tick_hz` | 写回 MSFS 的周期（`bindings/msfs/src/config.rs:156`） |
| `bridge.smoothing_mode` | `"low_latency"` | 取 `latest`、`low_latency`、`smooth`（`bindings/msfs/src/config.rs:161`、`:165`） |
| `bridge.interpolation_delay_ms` | 按模式取值 | `low_latency` 默认 30 ms，`smooth` 默认 80 ms（`bindings/msfs/src/config.rs:168-177`） |
| `bridge.max_extrapolation_ms` | 按模式取值 | `low_latency` 默认 40 ms，`smooth` 默认 20 ms（`bindings/msfs/src/config.rs:168-177`） |
| `bridge.stale_timeout_ms` | `500` | 判定实时状态陈旧的阈值（`bindings/msfs/src/config.rs:187`） |
| `bridge.enable_ai_aircraft` | `false` | 是否把其余飞机渲染成 AI 机（`bindings/msfs/src/config.rs:196`） |
| `bridge.max_ai_aircraft` | `8` | AI 机数量上限，最大 64（`bindings/msfs/src/config.rs:200`、`:265-267`） |
| `management.listen` | `"127.0.0.1:18003"` | 管理服务地址（`bindings/msfs/src/config.rs:211`） |
| `logging.level` | `"warn"` | 日志级别（`bindings/msfs/src/config.rs:242`） |

示例文件把 `interpolation_delay_ms` 与 `max_extrapolation_ms` 都写成 20，所以即使 `smoothing_mode` 仍是 `low_latency`，实际用的也是 20 ms 插值加 20 ms 外推。若显式给出 `aircraft_id`，它必须是 32 位十六进制 UUID，否则报 `aircraft_id must be a 32-character hexadecimal UUID`（`bindings/msfs/src/config.rs:268-272`）。

## 数据流

桥接是纯消费者侧的执行器，自身不读取 MSFS 的飞行数据，只把 FlyRuler 状态写进模拟器；仓库里没有从它发起的协议客户端连接。

1. 生产者（例如 `just msfs example` 运行的演示发送端，或接入协议库的自定义程序）通过 UDP 握手后上报 `AircraftState`。
2. 桥接把这些样本存进时间序列存储，渲染循环按 `1 / render_hz` 取样本（`bindings/msfs/src/bridge.rs:48`）。
3. 选机逻辑优先用配置里的 `aircraft_id`，否则取当前时刻已生成且最早出现的飞机（`bindings/msfs/src/lib.rs:162-194`）。
4. 实时模式下取最新样本并按 `stale_timeout_ms` 判断是否陈旧；回放模式下按游标取该时刻之前最近的样本（`bindings/msfs/src/aircraft.rs:246-265`）。
5. 样本按平滑模式插值或短时外推，转成 `MsfsFrame` 后写入 SimConnect（`bindings/msfs/src/lib.rs:388-417`）。

写入前，桥接会冻结 MSFS 对机位姿的解算，逐帧写机位姿、可选的机体速度与角速度、各操纵面和逐发动机油门；退出或飞机消失时解冻，把控制权还给模拟器（`bindings/msfs/src/aircraft.rs:41-67`、`:77-86`）。每 5 秒记录一次统计，包含缓冲区深度与接受、丢弃、最新、插值、外推、保持的帧数（`bindings/msfs/src/aircraft.rs:224-242`）。

## 字段映射

映射由 `frame_from_state` 与三个分解函数完成（`bindings/msfs/src/lib.rs:388-417`、`:491-559`）。机位姿里纬度、经度、海拔直接对应，姿态先用四元数转欧拉角再按 MSFS 约定取反：`pitch_rad = -pitch`、`bank_rad = -roll`、`heading_true_rad = yaw` 归一化到 `0..2π`。

| `AircraftState` 字段 | MSFS 仿真变量 | 单位 |
| --- | --- | --- |
| `derived.lat`、`derived.lon`、`derived.altitude` | `PLANE LATITUDE`、`PLANE LONGITUDE`、`PLANE ALTITUDE` | 度、度、米 |
| `attitude` 的欧拉角 | `PLANE PITCH DEGREES`、`PLANE BANK DEGREES`、`PLANE HEADING DEGREES TRUE` | 弧度 |
| `velocity` | `VELOCITY BODY X`、`VELOCITY BODY Y`、`VELOCITY BODY Z` | 米每秒 |
| `angular_velocity` | `ROTATION VELOCITY BODY X`、`Y`、`Z` | 弧度每秒 |
| `control_surfaces.aileron_left_rad`、`aileron_right_rad` | `AILERON LEFT DEFLECTION`、`AILERON RIGHT DEFLECTION` | 弧度 |
| `control_surfaces.elevator_rad`、`rudder_rad` | `ELEVATOR DEFLECTION`、`RUDDER DEFLECTION` | 弧度 |
| `control_surfaces.flaps_left_ratio`、`flaps_right_ratio` | `TRAILING EDGE FLAPS LEFT PERCENT`、`RIGHT PERCENT` | Percent Over 100 |
| `control_surfaces.spoilers_ratio` | `SPOILERS HANDLE POSITION` | Percent Over 100 |
| `propulsors[i].index` 为 1..4 时的 `throttle_ratio` | `GENERAL ENG THROTTLE LEVER POSITION:{index}` | Percent Over 100 |

SimConnect 变量名与单位定义在 `bindings/msfs/src/simconnect.rs:279-350`，操纵面到变量的对照在 `bindings/msfs/src/simconnect.rs:785-795`，发动机槽位按 `20 + index` 分配（`bindings/msfs/src/simconnect.rs:797-799`）。

机体速度与角速度的符号要经过一次坐标系转换：FlyRuler 用机体系前右下，MSFS 用右下前，于是 `VELOCITY BODY X` 取 `velocity.y`、`Y` 取 `-velocity.z`、`Z` 取 `velocity.x`，角速度同理（`bindings/msfs/src/lib.rs:521-543`）。缺 `derived` 或 `attitude` 的样本会被整帧丢弃，可选字段非法时只丢该字段并记录 `invalid optional aircraft field`（`bindings/msfs/src/lib.rs:420-473`）。比例类字段要求有限且落在 `0..=1`，角度类只要求有限。

## 起落架与 AI 机

起落架由事件驱动：桥接跟踪存储里的 `flyruler.control.gear_up` 与 `flyruler.control.gear_down` 事件，转换成 SimConnect 的 `GEAR_UP`、`GEAR_DOWN` 事件；换机或游标跳变时只应用最后一次命令，连续推进时应用区间内跨过的全部命令并去重（`bindings/msfs/src/lib.rs:216-312`）。

配置 `enable_ai_aircraft = true` 时，选中的飞机仍作为 MSFS 用户机，其余飞机用共享机模标题创建为非 ATC 的 AI 机，数量受 `max_ai_aircraft` 限制（`bindings/msfs/src/bridge.rs:137-156`）。

## 与控制台配合

桥接内嵌与 `fly-ruler-server` 相同的管理服务，因此控制台可以直接连它的 `management.listen`，使用同一套 `/api/v1` 接口查看飞机列表、曲线与回放，无需另起服务端（`bindings/msfs/src/bridge.rs:38-45`）。示例配置把管理地址设为 `0.0.0.0:18003`，远程浏览器也能访问；本地开发时把 Vite 的 `/api` 代理指到该端口即可。

桥接不向其它服务端转发状态。要让独立的 `fly-ruler-server` 同时记录同一批数据，需要生产者分别向两个地址上报。

## 常见失败原因

- 只看到 `waiting for MSFS 2024 SimConnect`：模拟器没启动、还停在主菜单，或不在 Free Flight 里；也可能 Active Pause 打开着。
- 构建期报找不到 `SimConnect.h` 或 `SimConnect.lib`：SDK 未解包或 `MSFS2024_SDK` 指错，按构建脚本要求的三处路径检查。
- 在 Linux 直接运行产物：得到的是给 Windows 编的可执行文件，必须经 Proton；本机交叉编译的 debug 产物用 `just msfs run` 跑。
- 日志出现 `aircraft state is stale; holding final MSFS pose`：生产端超过 `stale_timeout_ms` 没有新样本，飞机会保持最后一帧。
- 一直提示等待有效状态：客户端还没有完成握手并生成飞机；先启动一个生产者，例如 `just msfs example`。

配套的可运行示例是 `bindings/python/examples/07_msfs_client.py` 与 `bindings/python/examples/08_msfs_ai_fleet.py`；示例清单与运行方式见 `bindings/python/examples/README.md`。

## 相关页面

- [遥测与时间序列](/guide/components/proto/03-telemetry)
- [服务端与控制台](/guide/components/proto/05-server-and-console)
- [MSFS 桥接实现](/dev/components/proto/08-msfs-binding)
- [接口参考](/api/proto/)
