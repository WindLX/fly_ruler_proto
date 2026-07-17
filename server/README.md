# FlyRuler Server

`fly-ruler-server` is the standalone FlyRuler backend. It receives protobuf aircraft traffic over UDP and exposes the shared in-memory store, playback timeline, persistence operations, and read-only live snapshots over HTTP and WebSocket.

复制 `fly-ruler-server.example.toml` 为 `fly-ruler-server.toml` 后直接启动：

```bash
cp server/fly-ruler-server.example.toml fly-ruler-server.toml
cargo run -p fly_ruler_proto_server
```

未指定 `--config` 时，server 会自动读取当前工作目录中的 `fly-ruler-server.toml`；文件不存在时使用内置默认值。也可显式选择配置：

```bash
cargo run -p fly_ruler_proto_server -- --config ./deploy/server.toml
```

TOML 使用与当前 core `RuntimeConfig` 对齐的 `transport`、`management`、`playback` 和 `logging` section：

```toml
schema_version = 1

[transport]
udp_listen = "127.0.0.1:18002"
heartbeat_interval_secs = 5
heartbeat_timeout_secs = 15

[management]
enabled = true
listen = "127.0.0.1:18003"
data_root = "sessions"
web_root = "web/dist"
websocket_hz = 30.0

[playback]
default_speed = 1.0
min_speed = 0.1
max_speed = 16.0

[logging]
level = "info"
# file_path = "logs/fly-ruler-server.log"
```

相对路径以启动 server 时的当前工作目录为基准，与 MSFS bridge 一致。CLI 参数优先于 TOML，可用于部署时覆盖单个值：

```bash
cargo run -p fly_ruler_proto_server -- \
  --config ./fly-ruler-server.toml \
  --udp-listen 0.0.0.0:18002 \
  --http-listen 0.0.0.0:18003 \
  --log-level debug
```

两个 public URL 参数均可省略。默认生成同源 `/api/v1` 与 `/api/v1/ws`， 适合直接访问 `http://127.0.0.1:18003/`；只有通过反向代理或独立域名发布 前端时才需要覆盖。

Management 默认监听 loopback。若显式监听非 loopback 地址，应在反向代理层提供认证与 TLS，并通过 TOML `cors_origins` 或重复的 `--cors-origin` 明确允许浏览器来源。使用 `management.enabled = false` 或 `--no-http` 可只启动 UDP runtime。

The REST API is rooted at `/api/v1`; the WebSocket endpoint is `/api/v1/ws`. See [`core/README.md`](../core/README.md) for the endpoint and playback contract.

Proto 0.3 removed the old `engine_throttle` and `custom` series selectors. If an unreleased pre-0.3 workspace remains under `data_root/.fly-ruler/workspace.json`, `/api/v1/workspace` reports the obsolete selector instead of silently migrating it. Preserve and reset that development workspace with `mv sessions/.fly-ruler/workspace.json sessions/.fly-ruler/workspace.pre-0.3.json`; reloading the page creates a new workspace using `propulsor` and `telemetry` selectors.

Build the Vue dashboard with `just build-web`, then open `http://127.0.0.1:18003/`. For development, `just dev-console` starts the daemon and Vite together; Vite proxies HTTP and WebSocket API traffic to the daemon。生产构建的 `index.html` 是模板，启动时由 Rust 注入实际 API 地址。

Tag releases contain `fly-ruler-server-linux-x86_64.tar.gz` with this layout:

```text
fly-ruler-server/
├── fly-ruler-server
├── fly-ruler-server.example.toml
├── README.md
├── RELEASING.md
├── LICENSE
└── web/
    └── dist/
```

从解压目录运行 daemon 即可直接使用随包发布的 Web 控制台。完整发布流程见 [`../RELEASING.md`](../RELEASING.md)。
