# 安装与准备

本手册面向使用 FlyRuler 协议栈的人：用 Python 客户端把飞机状态推出去，让服务端接收、存储、展示，并驱动 Microsoft Flight Simulator 2024 里的飞机。日常使用中最重要的是 MSFS 桥自带的服务端，它既把状态写进模拟器，又在同一个端口上托管 web console。Rust 内核、wire schema 与各绑定的内部实现属于维护者话题，只放在开发手册里，例如 `docs/dev/01-architecture.md` 与 `docs/dev/advanced/wire-schema.md`。

## 三件套

| 组件 | 名字 | 职责 |
| --- | --- | --- |
| 客户端库 | `fly_ruler_proto_python` | Python 侧的 `FlyRulerClient`，把你构造的飞机状态打包成 UDP 报文推出去，用法见 `04-control.md` |
| 服务端 / 桥 | `fly-ruler-server`、`fly-ruler-msfs-bridge` | 接收状态、写入时间序列、提供 HTTP 与 WebSocket 管理接口；桥额外把状态写进 MSFS 2024 |
| 控制台 | `web/dist` | 随服务端同源托管的 Vue 前端，浏览器打开管理端口即可使用 |

桥与服务端共用同一个数据内核和管理服务，二者的 HTTP 接口都监听 `127.0.0.1:18003`（`bindings/msfs/src/bridge.rs:38-45`）。

## 用桥还是用独立服务端

只有要让 MSFS 2024 里的飞机跟着数据动，才需要桥。桥的 UDP 接收端口默认 `127.0.0.1:18002`，管理端口默认 `127.0.0.1:18003`，会话数据写在 `sessions/`，控制台资源取自 `web/dist`。

不接模拟器时用独立服务端 `fly-ruler-server` 就够了：它接收同一套 UDP 报文，提供同一套管理接口，默认监听 `127.0.0.1:18002` 与 `127.0.0.1:18003`（`server/src/config.rs:146`、`:157`），数据目录默认 `sessions`，控制台目录默认 `web/dist`（`server/src/config.rs:161`、`:167`）。

两者的客户端代码完全一样，你可以在不接模拟器时用独立服务端调协议，接上模拟器时换成桥。

## 平台前提

Windows 10/11 可以直接运行桥：解压发布包后执行 `fly-ruler-msfs-bridge.exe`，SimConnect 由本机的 MSFS 2024 提供。

Linux 上桥以 Windows 可执行文件的形式运行，由 Steam 版 Microsoft Flight Simulator 2024 的 Proton 兼容层承载，Steam AppID 是 2537590；发布包里的那个 exe 在 Linux 上原样使用，不需要重新编译。用安装脚本装好之后，日常只需要 `fly-ruler-msfs` 这一条命令，Proton 与 AppID 都由它带好。

桥的完整逻辑只在 Windows 目标上编译，其它目标编译出的程序会打印 `fly-ruler-msfs-bridge must be built for x86_64-pc-windows-msvc and run under Proton` 并以状态码 2 退出（`bindings/msfs/src/main.rs:10-24`）。

Python 侧要求 3.12 或更高（`bindings/python/pyproject.toml:9`）。

## 路线一：安装已发布的产物

Python 客户端库用 pip 装：

```bash
python -m pip install fly-ruler-proto-python
```

发行包名在 pip 里会做归一化处理，写连字符或下划线都能装；导入时始终写 `import fly_ruler_proto_python`。

Linux 上的 MSFS 桥用安装脚本装到用户空间，和常见命令行工具一样：

```bash
curl -fsSL https://raw.githubusercontent.com/WindLX/fly_ruler_proto/main/scripts/install-msfs.sh | bash
```

脚本从 GitHub Release 里取 `fly-ruler-msfs-windows-x86_64.zip`，校验压缩包内的 `SHA256SUMS` 之后装出这样一套目录：

| 位置 | 内容 |
| --- | --- |
| `~/.local/share/fly-ruler-msfs/versions/<版本>/` | 桥的可执行文件、`SimConnect.dll`、控制台资源 |
| `~/.local/share/fly-ruler-msfs/current` | 指向当前版本的软链，升级时原子替换 |
| `~/.local/share/fly-ruler-msfs/sessions/` | 会话数据 |
| `~/.config/fly-ruler-msfs/fly-ruler-msfs.toml` | 配置，里面的路径都是绝对路径，重装不会覆盖 |
| `~/.local/state/fly-ruler-msfs/` | 工作目录，桥从这个目录启动 |
| `~/.local/bin/fly-ruler-msfs` | 启动命令 |

常用选项：

| 选项 | 作用 |
| --- | --- |
| `--version v0.4.0` | 装指定版本，默认取最新 Release |
| `--with-service` | 额外写 `~/.config/systemd/user/fly-ruler-msfs.service`，只写文件，不 enable、不启动 |
| `--appid 2537590` | 换一个 MSFS 的 Steam AppID |
| `--dry-run` | 只解析版本、打印将要执行的动作，不动磁盘 |
| `--skip-checks` | 跳过 protontricks、MSFS 目录、端口占用等环境检查 |
| `--strict` | 把上面那些警告当成错误 |

装完运行 `fly-ruler-msfs`，它会把桥送进 MSFS 的 Proton 前缀；`~/.local/bin` 不在 `PATH` 里时脚本会提示怎么加。桥不是常驻服务：先启动 MSFS 2024 并进入 Free Flight，再运行这条命令，收工按 Ctrl-C。想临时放进后台就用 `systemctl --user start fly-ruler-msfs`，它写的 unit 是按需启动的，没有 `[Install]` 段，`systemctl --user enable` 会直接失败。

这段地址取的是主干上的脚本，因此不必等某个版本把脚本发成资产；要连脚本一起钉到某个版本，就换成标签地址，例如 `https://raw.githubusercontent.com/WindLX/fly_ruler_proto/vX.Y.Z/scripts/install-msfs.sh`，或者用同一个 Release 的资产地址 `https://github.com/WindLX/fly_ruler_proto/releases/download/vX.Y.Z/install-msfs.sh`（把 `vX.Y.Z` 换成标签即可）。`--version vX.Y.Z` 钉的是要装的桥版本，和脚本地址是两件事。

卸载还是这个脚本：`curl -fsSL https://raw.githubusercontent.com/WindLX/fly_ruler_proto/main/scripts/install-msfs.sh | bash -s -- --uninstall`，默认保留配置、日志与会话数据，加 `--purge` 连这些一起删。

Windows 上不用安装脚本：到 GitHub Release 页面取 `fly-ruler-msfs-windows-x86_64.zip`，解压后直接运行 `fly-ruler-msfs-bridge.exe`，SimConnect 由本机的 MSFS 2024 提供。不接模拟器时另取 `fly-ruler-server-linux-x86_64.tar.gz`，解压后在 `fly-ruler-server/` 目录里执行 `./fly-ruler-server`；两个压缩包都自带 `web/dist/` 控制台资源和示例 TOML 配置。

客户端与桥（或独立服务端）的 `PROTOCOL_VERSION` 必须严格相等，否则握手会被拒绝：服务端在握手分支里比较版本，不一致时回 `ProtocolVersionMismatch` 错误码与文本 `protocol version mismatch`（`core/src/transport/server.rs:445-467`）。当前值在本仓中是 `0.4.0`（`core/src/lib.rs:34`、`Cargo.toml:6`）。因此 Python 包与桥/服务端压缩包要从同一个 Release tag 取；跨版本混用一定连不上。

## 路线二：从源码构建

在 `packages/fly_ruler_proto` 目录内依次执行：

1. `just setup` 安装 Python 与 Web 依赖（`justfile:12`）。
2. `just build` 构建 Rust workspace 与控制台产物（`justfile:24`）。
3. `cd bindings/python && uv sync --all-groups && uv run maturin develop` 装好本地 Python 绑定（等价于 `justfile:82-83` 与 `justfile:139-140`）。

如果要自己交叉编译桥，Linux 上还需要准备这些前置条件：

- `cargo install cargo-xwin`；
- `rustup target add x86_64-pc-windows-msvc`；
- 系统里有 `llvm-lib`，发行版常把它装在带版本号的名字下，需要自己建一个不带版本号的入口，例如 `sudo ln -sf /usr/bin/llvm-lib-18 /usr/local/bin/llvm-lib`；
- `uv tool install protontricks`，用来在 Proton 里启动 exe。

构建脚本还需要 MSFS 2024 SimConnect SDK：它优先把环境变量 `MSFS2024_SDK` 当作 SDK 根，没有设置时回落到 `<manifest>/../../.msfs2024-sdk/MSFS 2024 SDK`（`bindings/msfs/build.rs:5-14`），其中 `manifest` 是 `bindings/msfs`，所以回落路径正好是本仓根下的 `.msfs2024-sdk/MSFS 2024 SDK`。SDK 里必须有 `SimConnect SDK/include/SimConnect.h`、`SimConnect SDK/lib/SimConnect.lib` 和 `SimConnect SDK/lib/SimConnect.dll`，缺任何一个构建脚本都会报 `missing MSFS 2024 SDK file: ... (set MSFS2024_SDK to the SDK root)`（`bindings/msfs/build.rs:33-39`）。准备好之后 `just msfs build` 会用 `cargo xwin` 交叉编译到 `x86_64-pc-windows-msvc`（`justfile:148-149`）。

源码构建完成后不必先打包就能跑：`just dev server` 直接运行独立服务端（`justfile:31-32`），适合不接模拟器时调协议；`just dev all` 会同时起服务端与 Vite 开发服务器（`justfile:37-42`）。桥则走 `just msfs run`，它假定你已经用 `just msfs build` 生成了 debug 产物，并且用仓库里的 `bindings/msfs/fly-ruler-msfs.dev.toml` 作为配置（`justfile:158-159`）；想换成自己的配置就设 `FR_MSFS_CONFIG` 指向那个文件。

## 确认两端版本一致

装完先对一下协议版本，再连：

- Python 客户端：`cd bindings/python && uv run fly_ruler_proto_python`，第一行是 `fly_ruler_proto_python 协议版本：...`（`bindings/python/src/fly_ruler_proto_python/__init__.py:60-67`）。也可以写 `python -c "import fly_ruler_proto_python as f; print(f.PROTOCOL_VERSION)"`。
- 桥或独立服务端：启动之后请求 `curl -s http://127.0.0.1:18003/api/v1/health`，返回 `{"status":"ok","protocol_version":"...","api_version":"v1"}`（`core/src/management/routes.rs:75-80`）。

两处输出必须完全一样。如果只改了其中一处版本号，`just check` 里的 `_check-version` 会抓住 `Cargo.toml`、`core/src/lib.rs` 与 `web/package.json` 三处不一致并报错（`justfile:98-112`）。

## 相关页面

- [五分钟跑通](02-quickstart.md)
- [排障](07-troubleshooting.md)
