# FlyRuler Server

`fly-ruler-server` is the standalone FlyRuler backend. It receives protobuf
aircraft traffic over UDP and exposes the shared in-memory store, playback
timeline, persistence operations, and read-only live snapshots over HTTP and
WebSocket.

```bash
cargo run -p fly_ruler_proto_server -- \
  --udp-listen 127.0.0.1:8080 \
  --http-listen 127.0.0.1:8081 \
  --data-root ./sessions \
  --ws-hz 30
```

The management listener is intentionally restricted to loopback addresses.
Additional browser origins can be supplied by repeating
`--cors-origin http://localhost:PORT`.

The REST API is rooted at `/api/v1`; the WebSocket endpoint is
`/api/v1/ws`. See [`core/README.md`](../core/README.md) for the endpoint and
playback contract.
