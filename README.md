# FlyRuler Protocol

FlyRuler 的 protobuf/UDP 数据内核与多语言绑定。项目接收高频飞行器状态， 在内存中维护可持久化时序数据，并通过 HTTP、WebSocket 和 Web 控制台提供 实时查看、历史曲线、事件导航、会话保存以及全局回放。

## 快速开始

```bash
# Rust、Python 与 Web 依赖
just setup
cd web && pnpm install && cd ..

# 同时启动 Rust daemon 与 Vite 开发服务器
just dev-console
```

生产模式由 Rust 直接托管构建后的前端：

```bash
just web-build
just run-server
# 打开 http://127.0.0.1:18003/
```

Python 示例发送端：

```bash
just develop
cd bindings/python
uv run python examples/demo_client.py
```

## 组成

- `core/`：UDP 会话、时序 Store、Parquet 持久化、回放和管理 API。
- `server/`：独立 `fly-ruler-server` daemon。
- `web/`：Vue 3、Tailwind 4、Pinia 与 ECharts 管理台。
- `bindings/python/`：仿真/控制程序使用的 Python 客户端。
- `bindings/godot/`：Godot 4 GDExtension。
- `bindings/msfs/`：MSFS 2024 SimConnect bridge。
- `proto/`：唯一 protobuf wire schema。

默认地址为 UDP `127.0.0.1:18002` 和 HTTP/WS `127.0.0.1:18003`。 Web 控制台内部使用原始秒时间戳查询和 seek，但以数据起点为零展示相对时间， 并在 Unix 时间可用时同时显示本地绝对时间。

## 常用命令

```bash
just test
just check
just web-check
just build-msfs
```

MSFS SDK、Proton 启动和 TOML 配置见
[`bindings/msfs/README.md`](bindings/msfs/README.md)。管理 API 与存储格式见
[`core/README.md`](core/README.md)，总体设计见 [`arch.md`](arch.md)。
