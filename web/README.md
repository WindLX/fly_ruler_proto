# FlyRuler Web Console

Vue 3 management and replay console for the FlyRuler HTTP/WebSocket runtime.
It provides live aircraft state, session operations, global playback controls,
drag/resizable ECharts canvases, LTTB-backed history queries, and persistent
curve/layout styling.

```bash
pnpm install
pnpm dev
pnpm check
pnpm build
```

The Vite development server proxies `/api` and `/api/v1/ws` to
`127.0.0.1:8081`. Production assets are written to `web/dist` and served by the
Rust management runtime when `ManagementConfig.web_root` points there.
