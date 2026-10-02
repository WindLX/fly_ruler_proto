# 扩展点与验证

这一页给出改动这个仓库时的固定路线：协议字段怎么加、core 里要动哪几处、各语言绑定按什么顺序同步、测试应该补在哪里。按顺序走可以避免"wire 层通了但某个绑定看不到字段"这类半成品状态。

## 一、改 schema

`core/proto/fly_ruler.proto` 是 wire schema 的唯一事实源，包名 `flyruler`（`core/proto/fly_ruler.proto:3`）。加字段时只做两件事：给新字段分配一个**从未使用过**的编号，并写清单位与符号约定。已有编号永远不复用，因为反序列化会忽略未知字段（`core/src/pb.rs:47` 的用例用一段旧报文验证了这一点）。

改完 schema 不需要手工生成：`core/build.rs:6-8` 注册了 `rerun-if-changed`，下一次 `cargo build` 会用 vendored protoc 走 prost-build 重新生成，落到 `core/src/pb.rs:8` 的 `include!` 目标里。生成的代码不可手改。

## 二、让 core 认识它

生成类型之后，按用途决定改动点：

- 参与时间序列存储与查询：`core/src/store.rs`（`AircraftTimeSeries`、`TimeSeriesStore`）与 `core/src/management/series.rs`（目录与分页接口）。
- 参与 UDP 会话收发：`core/src/transport/client.rs` 与 `core/src/transport/server.rs`，注意 ACK、心跳与 best-effort 语义，回归测试在 `core/src/transport/server.rs` 与 `core/src/transport/client.rs` 内。
- 参与回放：`core/src/playback.rs` 与 `core/src/cursor.rs`（游标帧组装与订阅）。
- 对外暴露成管理接口：`core/src/management/routes.rs`，并在 `core/src/management/gate.rs` 判断是否允许写入；会话级服务生命周期在 `core/src/management/server.rs`。

## 三、同步各语言绑定

Python 是最完整的一层，同步清单固定为四项：`bindings/python/src/protocol.rs` 的 `pyclass` 与 `From<PyX> for pb::X` 转换、`bindings/python/src/lib.rs` 的 `#[pymodule]` 注册、`bindings/python/src/fly_ruler_proto_python/_core.pyi` 存根、以及需要默认值时 `bindings/python/src/fly_ruler_proto_python/client.py` 的封装与 docstring。

Godot 绑定按机型配置与模型分层：`bindings/godot/src/model.rs` 与 `bindings/godot/src/aircraft_profile.rs` 读字段构造可显示状态，`bindings/godot/src/lib.rs` 是 GDExtension 入口。

MSFS 桥要同时改读取映射与写回展开：`frame_from_state()`（`bindings/msfs/src/lib.rs:388`）、`frame_control_values()`（`bindings/msfs/src/lib.rs:576`），离散事件还要过 `GearEventTracker`（`bindings/msfs/src/lib.rs:198`）。

遥测类字段不用改数据类结构，但必须让声明、采集与目录三处一致：`TelemetryField`（`bindings/python/src/protocol.rs:336`）与 `TelemetryStreamSchema`（`bindings/python/src/protocol.rs:384`）负责声明，core 侧写入走 `TelemetryFrame`，目录由 `core/src/management/series.rs` 提供。

## 四、版本与兼容

协议语义变更（字段含义、单位、必填性变化）必须同步版本号：`PROTOCOL_VERSION`（`core/src/lib.rs:34`）、workspace 版本（`Cargo.toml:6`）与 `web/package.json:4` 三处由 `just set-version X.Y.Z` 一次改齐，脚本实现见 `scripts/update_version.py`。`just _check-version` 会在三处不一致时直接失败，`just check` 已经包含它。

只新增可选字段、不改既有字段语义时不需要动版本号；这是 proto3 下唯一安全的加字段方式。

## 五、测试放哪

测试跟着实现走，就近放在同一文件：

- core：`core/src/store.rs`（存储与分页）、`core/src/playback.rs`（回放步进）、`core/src/cursor.rs`（游标帧）、`core/src/management/series.rs`（目录）、`core/src/management/server.rs`（页面托管与配置注入）、`core/src/transport/*.rs`（会话与 ACK）。
- 生成的 wire 类型：`core/src/pb.rs`，用往返编解码与未知字段忽略两类用例覆盖。
- Python：`bindings/python/tests/test_core.py`，覆盖公开面、构造校验与版本一致性。
- 前端：`web/src/**/*.test.ts`，其中 `web/src/stores/stores.test.ts` 覆盖三个 store 的动作与状态迁移。
- MSFS：`bindings/msfs/src/lib.rs`、`bindings/msfs/src/smoothing.rs`、`bindings/msfs/src/config.rs`，必须是不依赖 SimConnect 的运行环境也能跑的部分。

## 六、提交前的门禁

本机默认验证是 `just check` 与 `just test`，它们覆盖 Rust、Python 与前端；涉及 MSFS 或 Godot 时再显式跑 `just msfs check`、`just msfs build`。发布前的完整核对是 `just check-release`，它在本机门禁之外还包括 MSFS 的交叉检查与打包。

改动跨语言时最容易漏的是文档：公开面变化要同步更新 `docs/dev/` 对应章节与 `docs/dev/api.md` 的对象表，接口参考页由源码 docstring 生成，改完注释需要重新生成。

## 相关页面

- [wire schema 与协议版本](/dev/components/proto/02-wire-schema)
- [UDP 会话语义](/dev/components/proto/03-udp-session)
- [Python 绑定实现](/dev/components/proto/06-python-binding)
- [接口参考](/dev/components/proto/api)
