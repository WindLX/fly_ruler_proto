# 五分钟跑通

这一节从零走到看着飞机在 MSFS 2024 里动起来，再到控制台里看到数据。整条路径按「模拟器就绪 → 启动桥 → 推数据 → 打开控制台」推进，每一步都有可以当场看到的输出；卡住时按 `07-troubleshooting.md` 排查。

## 开始之前

先把这几件事准备好：

1. Python 3.12 或更高（`bindings/python/pyproject.toml:9`），客户端库按 `01-install.md` 装好。
2. MSFS 2024 已经能正常进入飞行；桥用 `01-install.md` 的安装脚本装好（或者已经自己构建/解压出 exe），它与 Python 包来自同一个 Release tag。
3. 本机 UDP 端口 18002 与管理端口 18003 没有被别的进程占用。

## 数据怎么流动

桥这一个进程里同时跑着几件事：UDP 接收端在 `127.0.0.1:18002` 收客户端报文，数据内核把每架飞机写进内存时间序列，管理服务在 `127.0.0.1:18003` 对外提供 HTTP、WebSocket 和控制台；桥再按渲染步长从内存里取当前帧，通过 SimConnect 写进 MSFS（`bindings/msfs/src/bridge.rs:29-45`、`:79-159`）。

所以控制台里的曲线和模拟器里飞机的动作来自同一份数据，一端看到的异常可以用另一端交叉印证。

## 1 在 MSFS 里进入 Free Flight

启动 Microsoft Flight Simulator 2024，选好机模和机场，进入 Free Flight，飞机停在地面或已经在空中都可以。

关键是把 Active Pause 关掉：桥靠 SimConnect 把每帧姿态写进用户机，模拟器一旦处于 Active Pause，画面不会随数据更新，你会误以为桥没有工作。

## 2 启动桥

装过安装脚本就一条命令：

```bash
fly-ruler-msfs
```

它读取 `~/.config/fly-ruler-msfs/fly-ruler-msfs.toml`，在 MSFS 2024 的 Proton 前缀里启动桥，安装步骤见 `01-install.md`。从源码跑等价于 `just msfs run`，它用仓库里的 `bindings/msfs/fly-ruler-msfs.dev.toml` 指向 debug 构建产物（`justfile:158-159`）。Windows 上直接运行发布包里的 `fly-ruler-msfs-bridge.exe`，它会自己去找本机的 MSFS。

桥不是常驻服务，跑在前台，收工按 Ctrl-C；因为没有安装脚本而想手工进前缀时，也可以自己执行 `protontricks-launch --appid 2537590 ./fly-ruler-msfs-bridge.exe`。

桥启动时先绑 UDP 接收端口 `127.0.0.1:18002`（`bindings/msfs/src/bridge.rs:29-32`），再起管理面 `127.0.0.1:18003`（`bindings/msfs/src/bridge.rs:38-45`）。SimConnect 一时连不上不会致命，它会每秒重试并打印一行 `waiting for MSFS 2024 SimConnect`（`bindings/msfs/src/bridge.rs:63-71`）。默认日志级别是 `info`，只想要警告就把配置里的 `logging.level` 改成 `warn`，或者启动时加 `--log-level warn`（`bindings/msfs/src/config.rs:60`、`:242`，常量见 `core/src/logging.rs:13`）。日志正文形如下面几行（时间戳与线程名已省略）：

```text
 INFO fly_ruler_proto_msfs.bridge: MSFS bridge configuration loaded config_path=Some("/home/you/.config/fly-ruler-msfs/fly-ruler-msfs.toml")
 INFO fly_ruler_proto_msfs.bridge: FlyRuler UDP server started addr=127.0.0.1:18002
 INFO fly_ruler_proto_msfs.bridge: FlyRuler HTTP/WebSocket management server started addr=127.0.0.1:18003
 INFO fly_ruler_proto_msfs.bridge: MSFS bridge live smoothing configured tick_hz=240.0 render_hz=240.0 ...
 WARN fly_ruler_proto_msfs.bridge: waiting for MSFS 2024 SimConnect
 INFO fly_ruler_proto_msfs.bridge: SimConnect connected; waiting for a valid FlyRuler aircraft state
```

看到 `SimConnect connected` 那一行之后就可以推送数据了；桥会一直等到有飞机状态到达才真正驱动模拟器。

## 3 让飞机动起来

服务端已经随桥一起跑起来了，现在另开一个终端，用 Python 客户端推一条圆周航迹：

```bash
cd bindings/python
uv run python examples/02_control_msfs.py
```

这个示例以 60 Hz 推送，飞机绕 `--latitude 31.1434`、`--longitude 121.8052` 上方半径 500 m 的圆飞行，默认高度 1200 m、地速 70 m/s；每一帧都带派生空气数据、七个操纵面和两台发动机，并且每 8 秒用 `flyruler.control.gear_up` / `flyruler.control.gear_down` 事件切换一次起落架（`bindings/python/examples/02_control_msfs.py:52-108`，参数默认值见 `:111-132`）。

也可以直接 `just msfs example`，它跑的就是上面这条命令（`justfile:161-162`）。

## 4 打开控制台

浏览器打开 `http://127.0.0.1:18003`，这就是桥托管的控制台，和 UDP 接收端同源。左栏能看到刚推上来的那架飞机，中栏可以拉出曲线，底部时间轴会随着数据前进。界面各区域的用途见 `03-console.md`。

控制台连的是同一份内存数据，所以暂停、回放、保存会话都不需要重启桥，也不会打断客户端的推送。

## 5 常见卡点

- 客户端报连接失败：先确认桥已启动，再核对两端的 `PROTOCOL_VERSION`，做法见 `01-install.md`。
- 提示找不到 `fly-ruler-msfs`：`~/.local/bin` 不在 `PATH` 里，按安装脚本的提示把 `export PATH="$HOME/.local/bin:$PATH"` 加进 shell 配置。
- 桥一直打印 `waiting for MSFS 2024 SimConnect`：确认 MSFS 2024 已进入 Free Flight，Linux 上确认跑在 AppID 2537590 的 Proton 前缀里。
- 控制台有数据但飞机不动：确认 Active Pause 已关闭，客户端 `--address` 指向 `127.0.0.1:18002`。

更多症状与处理见 `07-troubleshooting.md`。

## 相关页面

- [安装与准备](01-install.md)
- [在 web console 里管理](03-console.md)
- [用 Python client 控制飞行](04-control.md)
