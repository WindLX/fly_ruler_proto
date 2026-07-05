# FlyRuler Web Console

Vue 3 management and replay console for the FlyRuler HTTP/WebSocket runtime.
It provides live aircraft state, session operations, global playback controls,
drag/resizable ECharts canvases, LTTB-backed history queries, and persistent
curve/layout styling.

The global timeline uses readable elapsed time, local wall-clock timestamps,
adaptive ticks, and merged spawn/despawn/custom event markers. Chart X axes
are relative to the global data start while REST queries and playback seek keep
the original timestamp seconds.

```bash
pnpm install
pnpm dev
pnpm check
pnpm build
```

The Vite development server proxies `/api` and `/api/v1/ws` to
`127.0.0.1:18003`. Production assets are written to `web/dist` and served by the
Rust management runtime when `ManagementConfig.web_root` points there.
The built `index.html` contains a runtime configuration placeholder. Rust
replaces it with `api_base_url` and `websocket_url`; do not replace the
placeholder during production builds. Vite injects same-origin development
defaults and continues to proxy `/api`.
