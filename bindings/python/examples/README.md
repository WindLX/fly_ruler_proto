# Python 客户端示例

这组脚本从最简连接到多机编队依次递进，每个脚本都能独立运行，只需要一个在跑的服务端。

## 运行前置

在 `bindings/python` 下同步依赖并本地安装扩展：

```bash
cd bindings/python
uv sync --all-groups
uv run maturin develop
```

另开一个终端启动服务端；需要喂给模拟器时改为启动 MSFS 桥接：

```bash
just dev server
# 或：just msfs run
```

然后在 `bindings/python` 下运行任意一个脚本：

```bash
uv run python examples/01_connect_and_push.py
```

服务端没有运行时脚本会打印中文原因，并以退出码 1 结束。

## 阅读顺序

| 脚本 | 内容 | 对应章节 |
| --- | --- | --- |
| `01_connect_and_push.py` | 握手、协议版本、两个会话 UUID，随后按固定频率推送状态 | `docs/guide/02-quickstart.md` |
| `02_control_msfs.py` | 地理圆航迹、派生量、控制面与发动机，喂给 MSFS 桥接 | `docs/guide/04-control.md` |
| `03_events_and_telemetry.py` | 注册遥测流并发布数据帧，同时发自定义事件与起落架事件 | `docs/guide/05-telemetry.md` |
| `04_sessions_and_playback.py` | 长时圆周飞行与事件标记，配合控制台观察回放与时间轴 | `docs/guide/06-sessions.md` |
| `05_multi_aircraft.py` | 多个客户端、编队上报与统一关闭 | `docs/guide/03-console.md` |
| `06_ai_fleet_msfs.py` | MSFS 桥接的多架 AI 飞机编队 | `docs/guide/04-control.md` |

## 常用参数

每个脚本都用 `argparse`，公共参数保持一致：

| 参数 | 含义 |
| --- | --- |
| `--address` | 服务端 UDP 地址，默认 `127.0.0.1:18002` |
| `--aircraft` / `--prefix` | 飞机显示名或编队名前缀 |
| `--hz` | 上报频率，默认 30 Hz（MSFS 示例 60 Hz） |
| `--duration` | 运行时长，`0` 表示一直运行到中断 |

MSFS 相关脚本还保留了 `--latitude`、`--longitude`、`--radius`、`--engine-count`、`--fleet-size`、`--spacing` 等专属参数，用 `--help` 查看。

## 退出码

- 连不上 UDP 服务端时，脚本打印中文原因并以退出码 `1` 结束。
- `Ctrl-C` 会走完 `finally`，先把所有客户端关掉，再退出。
- 正常运行到 `--duration` 结束以退出码 `0` 结束。

## 观察结果

状态、事件与遥测写入服务端后，用控制台查看曲线、回放与时间轴：

```bash
just dev all
```

浏览器打开 `http://127.0.0.1:5173`（开发模式），或直接访问服务端托管地址 `http://127.0.0.1:18003`。
