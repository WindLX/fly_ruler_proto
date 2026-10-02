# Python 客户端示例

这组脚本从最简连接到多机编队依次递进，每个脚本都能独立运行，只需要一个在跑的服务端：

```bash
uv sync --all-groups
uv run maturin develop
just dev server            # 另开一个终端
uv run python examples/01_connect.py
```

没有服务端时脚本会打印中文原因；前六个脚本以退出码 1 结束，两个 MSFS 示例以退出码 0
结束，方便在没有模拟器环境的机器上试跑。

## 阅读顺序

| 脚本 | 内容 | 对应章节 |
| --- | --- | --- |
| `01_connect.py` | 握手、协议版本、两个会话 UUID | `docs/guide/01-install.md` |
| `02_state_update.py` | 按固定频率上报状态、时间戳语义 | `docs/guide/02-client-connection.md` |
| `03_events.py` | 自定义事件与起落架事件 | `docs/guide/02-client-connection.md` |
| `04_telemetry.py` | 注册遥测流、按字段顺序发布数据帧 | `docs/guide/03-telemetry.md` |
| `05_multi_aircraft.py` | 多个客户端、编队上报与统一关闭 | `docs/guide/03-telemetry.md` |
| `06_circle_demo.py` | 圆周飞行、事件标记、长时运行 | `docs/guide/04-playback.md` |
| `07_msfs_client.py` | 带派生量、控制面、发动机的地理航迹 | `docs/guide/06-msfs-bridge.md` |
| `08_msfs_ai_fleet.py` | 多架 AI 飞机的编队 | `docs/guide/06-msfs-bridge.md` |

## 常用参数

每个脚本都用 `argparse`，公共参数保持一致：

| 参数 | 含义 |
| --- | --- |
| `--address` | 服务端 UDP 地址，默认 `127.0.0.1:18002` |
| `--aircraft` / `--prefix` | 飞机显示名或编队名前缀 |
| `--hz` | 上报频率，默认 30 Hz（MSFS 示例 60 Hz） |
| `--duration` | 运行时长，`0` 表示一直运行到中断 |

`Ctrl-C` 会走完 `finally`：先把所有客户端关掉，再退出。

## 观察结果

状态与事件写入服务端的时间序列后，用控制台查看曲线与回放：

```bash
just dev all
```

浏览器打开 `http://127.0.0.1:5173`，在左侧数据栏选择飞机与字段即可。
