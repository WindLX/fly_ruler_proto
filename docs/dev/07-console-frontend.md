# 控制台前端实现

控制台是随服务端一起分发的 Vue 3 单页应用：源码在 `web/src`，构建产物由管理服务直接托管。这一页讲清运行配置是怎么注入的、状态如何同步、图表为什么能扛住长时间序列，以及本地开发时前后端如何对接。

## 目录与职责

`web/src` 分成四层：

- 运行时与接口：`web/src/runtime.ts`（运行配置读取）、`web/src/api.ts`（REST 封装与 WebSocket 地址）、`web/src/types.ts`（管理接口的响应类型）。
- 状态：`web/src/stores/server.ts`、`web/src/stores/workspace.ts`、`web/src/stores/series.ts`。
- 视图：`web/src/App.vue` 组合 `components/DataSidebar.vue`（左侧数据篮）、`components/ChartWorkspace.vue` 与 `components/ChartCard.vue`（图表网格）、`components/InspectorPanel.vue`（单点详情）、`components/TimelineBar.vue` 与 `components/PlaybackToolbar.vue`（时间轴与回放控制）。
- 交互与文本：`web/src/shortcuts.ts`、`web/src/usePlaybackShortcuts.ts`、`web/src/i18n.ts`、`web/src/utils.ts`、`web/src/style.css`（Tailwind v4）。

## 运行配置的注入路径

前端不硬编码后端地址。构建产物里 `index.html` 留一个运行配置标记，部署它的进程在返回页面时把它替换成实际配置。

开发服务下，Vite 插件在 `serve` 模式把该标记替换为 `{api_base_url: '/api/v1', websocket_url: '/api/v1/ws'}`（`web/vite.config.ts:7-18`），并把 `/api` 代理到 `http://127.0.0.1:18003`、开启 WebSocket 转发（`web/vite.config.ts:28-37`）。

生产部署下，管理服务读取静态根目录的 `index.html`，要求其中包含哨兵字符串 `__FLY_RULER_RUNTIME_CONFIG__`（`core/src/management/server.rs:47`），再用实际监听地址替换后发给浏览器（`core/src/management/server.rs:407-429`）。这也是把控制台交给 `fly-ruler-server` 托管时唯一需要的对接点。

浏览器侧读取发生在模块加载时：`readRuntimeConfig()` 从 `#fly-ruler-runtime-config` 元素解析 JSON，缺失或仍是标记文本时退回默认值（`web/src/runtime.ts:11-23`）；WebSocket 地址由 `resolveWebSocketUrl()` 按当前页面协议把 `http`/`https` 换成 `ws`/`wss`（`web/src/runtime.ts:29-34`）；`runtimeConfig` 常量在模块顶层完成解析（`web/src/runtime.ts:36`）。

## 接口层

`web/src/api.ts:23` 的 `apiFetch()` 是所有请求的唯一出口，统一处理基地址、JSON 解析与错误码；`api` 对象在 `web/src/api.ts:38` 暴露各管理端点；`websocketUrl()` 在 `web/src/api.ts:114` 把配置里的地址转成 WebSocket 连接串。

## 状态同步

`stores/server.ts` 持有连接态与服务端快照：`connected`、`status`、`aircraft`、`sessions`、`samples`、`operations`、`timelineEvents`（`web/src/stores/server.ts:17-29`）。`connect()`（`web/src/stores/server.ts:70`）建立 WebSocket 并按消息类型分发，`stop()`（`:144`）断开，错误统一走 `reportError()`（`:175`）。快照与事件都来自推送，因此界面刷新频率由服务端决定。

`stores/workspace.ts` 管多图布局与本地编辑：布局项形如 `{i, x, y, w, h}`，另有曲线篮子、选中飞机与 `dirty` 标记（`web/src/stores/workspace.ts:14-24`）。本地修改通过 `watch(workspace, scheduleSave, {deep: true})`（`:196`）防抖写回服务端，`scheduleSave()` 在 `:67`；服务端 revision 变化走 `handleRemoteRevision()`（`:77`），用它区分"自己的保存回声"和"别人的修改"，避免互相覆盖。图表增删与篮子操作集中在 `addToBasket()`（`:124`）、`createChart()`（`:130`）、`addBasketToSelected()`（`:148`）、`removeChart()`（`:158`）、`updateLayout()`（`:165`）、`updateChartView()`（`:178`）。

`stores/series.ts` 管时间序列数据：按曲线签名分别缓存目录与数据（`web/src/stores/series.ts:8-16`），并用请求代数丢弃过期响应。长时间序列靠两级处理保持流畅：`downsampleSeriesData()`（`:128`）用 LTTB 把点数压到上限，`mergeSeriesData()`（`:179`）把增量帧合并进已有序列并按时间戳去重，保证回放拖动与实时追加都不会出现重复点。

## 构建、测试与调试

`pnpm check` 串起 `format:check`、`lint`、`test`、`build`（`web/package.json:15`）；`build` 先跑 `vue-tsc -b` 做类型检查再 `vite build`（`web/package.json:9`）。单元测试用 Vitest，运行在 node 环境（`web/vite.config.ts:38-40`），覆盖运行配置解析、快捷键、多语言、工具函数与三个 store（`web/src/stores/stores.test.ts`）。

本地对接后端有两种方式：用 `just dev web` 起 Vite 开发服务，让 `/api` 代理到已在运行的管理服务；或者先构建前端再由服务端托管页面。前者支持热更新，后者验证的是真实部署路径。

## 扩展新面板

新增管理端点的完整改动面是：`core/src/management/routes.rs` 注册路由与响应结构、`web/src/types.ts` 补类型、`web/src/api.ts` 加请求方法、对应的 store 补状态与动作、最后在 `App.vue` 或某个组件里消费。任何一步缺失都会表现成"接口通了但界面没反应"，因此测试要同时覆盖 store 动作与渲染路径。

## 相关页面

- [服务端的 HTTP 与 WebSocket 接口](/dev/components/proto/05-server-http-ws)
- [服务端与控制台使用](/guide/components/proto/05-server-and-console)
- [时间序列存储与回放](/dev/components/proto/04-storage-playback)
- [接口参考](/dev/components/proto/api)
