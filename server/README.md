# FlyRuler Server

`fly-ruler-server` is the standalone FlyRuler backend. It receives protobuf aircraft traffic over UDP and exposes the shared in-memory store, playback timeline, persistence operations, and read-only live snapshots over HTTP and WebSocket.

```bash
cargo run -p fly_ruler_proto_server -- \
  --udp-listen 127.0.0.1:18002 \
  --http-listen 127.0.0.1:18003 \
  --data-root ./sessions \
  --web-root ./web/dist \
  --public-api-base-url https://sim.example.test/api/v1 \
  --public-websocket-url wss://sim.example.test/api/v1/ws \
  --ws-hz 30
```

两个 public URL 参数均可省略。默认生成同源 `/api/v1` 与 `/api/v1/ws`， 适合直接访问 `http://127.0.0.1:18003/`；只有通过反向代理或独立域名发布 前端时才需要覆盖。

The management listener is intentionally restricted to loopback addresses. Additional browser origins can be supplied by repeating `--cors-origin http://localhost:PORT`.

The REST API is rooted at `/api/v1`; the WebSocket endpoint is `/api/v1/ws`. See [`core/README.md`](../core/README.md) for the endpoint and playback contract.

Build the Vue dashboard with `just web-build`, then open `http://127.0.0.1:18003/`. For development, `just dev-console` starts the daemon and Vite together; Vite proxies HTTP and WebSocket API traffic to the daemon。生产构建的 `index.html` 是模板，启动时由 Rust 注入实际 API 地址。
