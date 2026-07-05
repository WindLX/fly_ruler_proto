# FlyRuler Web Console

Vue 3 management and replay console for the FlyRuler HTTP/WebSocket runtime. It provides live aircraft state, session operations, global playback controls,
drag/resizable ECharts canvases, LTTB-backed history queries, and persistent
curve/layout styling. Its compact, docked workbench uses flat editor panes,
narrow headers, high-density controls, and matched light/dark themes.

The global timeline uses readable elapsed time, local wall-clock timestamps,
adaptive ticks, and merged spawn/despawn/custom event markers. Chart X axes
are relative to the global data start while REST queries and playback seek keep
the original timestamp seconds.

Playback keyboard controls are active whenever focus is not inside an input,
select, editable field, or modal dialog:

| Shortcut            | Action                                                  |
| ------------------- | ------------------------------------------------------- |
| `Space`             | Pause Live/playing playback, or resume paused replay    |
| `←` / `→`           | Previous/next globally unique aircraft sample timestamp |
| `Shift` + `←` / `→` | Step ten sample timestamps                              |
| `↑` / `↓`           | Previous/next global lifecycle or custom event          |
| `Home` / `End`      | Seek to the global data bounds                          |

The timeline observes its rendered width and chooses labeled major ticks plus
shorter minor ticks from millisecond through multi-day ranges. Event clustering
is pixel-based, so marker density remains stable when sidebars are collapsed or
the window is resized.

Workspace data is reconciled whenever the in-memory Store or loaded Session
changes. Query windows that no longer overlap the active data are reset to the
full range, partially overlapping windows are clamped, and legacy `null` chart
zoom values are removed. When a Session contains one aircraft, stale curve
bindings automatically follow it; for multi-aircraft Sessions the chart
inspector provides an explicit aircraft selector.

The interface is fully localized in Chinese and English. Standard field names
and groups are translated by selector, while custom field identifiers remain
unchanged. Each chart reports loading, empty-range, unavailable-aircraft, and
request-error states instead of silently rendering an empty canvas.

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

The release workflow builds this directory once and passes the same `web-dist`
artifact to both packaging jobs:

- Linux server: `fly-ruler-server/web/dist`
- MSFS bridge: `fly-ruler-msfs/web/dist`

Both Rust binaries dynamically render the bundled `index.html`; the frontend is
not embedded into either executable. See [`../RELEASING.md`](../RELEASING.md)
for the verified archive layouts.
