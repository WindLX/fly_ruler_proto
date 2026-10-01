# fly_ruler_proto_server

`fly_ruler_proto_server` 是协议内核的独立 UDP 服务端（Rust，tokio 运行时，二进制名 `fly_ruler_proto_server`）。它创建一个 `TimeSeriesStore`，用 `KernelRuntime` 在配置的地址上启动 UDP 服务，并把收到的遥测写进时序存储，供 plot 与协议客户端读取。

## 目录结构

- `src/main.rs` —— 二进制入口：加载配置、初始化日志、启动 UDP 服务。
- `src/config.rs` —— 参数与配置解析（`Args`、`ServerConfig`），`--config` 指定 TOML 路径。
- `fly-ruler-server.example.toml` —— 配置模板，含运行时日志与 `udp_listen` 等字段。

## 运行

```bash
just run-server --config server/fly-ruler-server.toml
```

`just run-server` 等价于 `cargo run -p fly_ruler_proto_server -- <参数>`；不带参数时使用默认配置，实际监听地址与日志级别由配置文件决定。
