# fly-ruler-proto-web

`fly-ruler-proto-web` 是协议内核的 Web 控制台（Vue 3 + Vite + Pinia + ECharts），用时间轴回放遥测、按通道绘图，并能在同一工作区里比对多条曲线。它通过 HTTP 与 WebSocket 连到内核，前端不做协议解析。

## 目录结构

- `src/runtime.ts` —— 读取页面注入的 `fly-ruler-runtime-config`，给出 `api_base_url` 与 `websocket_url`（默认 `/api/v1`、`/api/v1/ws`）。
- `src/api.ts` —— 内核 HTTP 端点的薄封装，并提供 `websocketUrl()`。
- `src/stores/` —— 三个 Pinia store：`server`（连接与状态）、`workspace`（布局与选中项）、`series`（曲线数据）。
- `src/components/` —— `DataSidebar`、`ChartCard`、`ChartWorkspace`、`PlaybackToolbar`、`TimelineBar`、`InspectorPanel`。
- `src/types.ts` —— 前端视图模型与共享类型。
- `src/utils.test.ts`、`src/stores/stores.test.ts` —— vitest 用例。

## 开发与构建

- 安装依赖：`cd web && pnpm install`（仓库入口 `just setup`）。
- 开发：`pnpm dev`；生产构建：`pnpm build`（`vue-tsc -b && vite build`）。
- 测试与门禁：`pnpm test`（vitest）、`pnpm check`（格式检查、lint、测试、构建）。
