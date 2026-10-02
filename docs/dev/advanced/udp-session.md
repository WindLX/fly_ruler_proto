# UDP 会话与可靠性

这一页面向协议实现者与二次开发者，讲会话登记、心跳、ACK 与游标流的可靠性边界；只用 Python client 的用户请读使用手册 `docs/guide/01-install.md`。

一个飞行模型进程对应一个 `AircraftClient`，它把无连接的 UDP 数据报包装成一条有生命周期语义的会话：先握手登记身份，再用 spawn 声明飞机与遥测流，之后持续投递状态、事件与遥测帧并用心跳维持登记，最后以 despawn 或 close 结束（`core/src/transport/client.rs:295`）。服务端不持有客户端连接，只按源地址维护一张会话表，会话何时消失取决于心跳是否按时到达，因此客户端要区分被确认的写入与尽力而为的写入。

## 连接与握手

`Client::connect` 先绑定 `0.0.0.0:0` 取得临时端口，再对该地址调用 UDP `connect`（`core/src/transport/client.rs:162`），此后读写只面向服务端地址。

握手是一条 `Request` 信封下的 `Handshake`，携带 `PROTOCOL_VERSION`、随机生成的 client UUID 与 `ClientRole::Producer` 角色（`core/src/transport/client.rs:48-57`）。

`establish_session` 发出握手后等待响应，等待上限是 `HANDSHAKE_TIMEOUT`，即 1 秒（`core/src/transport/client.rs:18`、`:276-287`）。四类失败各有独立错误：

| 情况 | 错误 |
| --- | --- |
| 1 秒内没有响应 | `HandshakeTimeout`（`core/src/transport/client.rs:280-282`） |
| 收到错误响应 | `HandshakeRejected("code=… message=…")`（`core/src/transport/client.rs:266-269`） |
| 响应不是 `ACK=true` | `InvalidMessage("handshake response did not contain ACK=true")`（`core/src/transport/client.rs:270-272`） |
| 服务端提前关闭 | `InvalidMessage("server closed during handshake")`（`core/src/transport/client.rs:283-285`） |

服务端在接收循环里处理握手（`core/src/transport/server.rs:445-470`）：版本按严格相等判定，不等时回 `ErrorCode::ProtocolVersionMismatch` 与 `protocol version mismatch`（`core/src/transport/server.rs:455-465`）；角色解码失败或为 `Unspecified` 时回 `InvalidState` 与 `handshake role must be producer or cursor_subscriber`（`core/src/transport/server.rs:447-456`）；两者都通过且数据报带了 client UUID 才会 `set_session` 并回 ACK（`core/src/transport/server.rs:456-460`），缺少 UUID 的握手不会得到任何响应，客户端只能等到 1 秒超时。

同一 client UUID 从新地址再次握手时，旧地址的会话会被删除（`core/src/transport/server.rs:171-181`）。

## 消息构成

客户端发出的消息都由 `make_request` 包成 `Request` 信封，可选源时间戳写在信封上（`core/src/transport/client.rs:95-108`）。

| 消息 | 内容 | 构造函数 |
| --- | --- | --- |
| 握手 | `Handshake`，带协议版本、client UUID 与 producer 角色 | `build_handshake_message`（`core/src/transport/client.rs:48-57`） |
| 心跳 | `Heartbeat`，带序号与 client UUID | `build_heartbeat_message`（`core/src/transport/client.rs:59-67`） |
| spawn | `AircraftEvent > Spawn`，带飞机名、TOML 配置、初始状态与遥测流声明 | `build_spawn_message`（`core/src/transport/client.rs:69-93`） |
| 状态更新 | `AircraftEvent > StateUpdate` | `build_state_update_message`（`core/src/transport/client.rs:95-109`） |
| 自定义事件 | `AircraftEvent > CustomEvent`，载荷是事件名 | `build_custom_event_message`（`core/src/transport/client.rs:111-125`） |
| 遥测帧 | `AircraftEvent > TelemetryFrame` | `build_telemetry_frame_message`（`core/src/transport/client.rs:127-141`） |
| despawn | `AircraftEvent > Despawn`，带可选原因 | `build_despawn_message`（`core/src/transport/client.rs:143-159`） |

六类飞机事件都在 `AircraftEvent` 里填 `aircraft_id`，服务端按这个字段把消息归到对应飞机，来源地址只用于判断该地址是否已登记为 producer 会话（`core/src/transport/client.rs:101-103`、`core/src/transport/server.rs:503-513`）。

## spawn 与首包

握手成功后，操作任务先把一条 spawn 消息送进发送队列，再开始消费用户提交的操作（`core/src/transport/client.rs:424-436`），因此 spawn 总排在第一条状态更新之前。spawn 是 `AircraftEvent` 下的 `AircraftCommandInfo::Spawn`，带飞机名、TOML 配置、可选初始状态与遥测流声明（`core/src/transport/client.rs:69-88`）。

`connect_with_telemetry_at` 在建立会话前校验两件事：spawn 时间戳必须有限，心跳间隔必须有限且大于零，后者失败时返回 `InvalidMessage("heartbeat interval must be finite and greater than zero")`（`core/src/transport/client.rs:368-373`）。client UUID 与 aircraft UUID 各自随机生成（`core/src/transport/client.rs:374-375`），前者用于握手与心跳，后者用于该飞机的所有状态与事件。

服务端收到 spawn 后先校验遥测流声明，任一项不合法就整条丢弃并记 warn，飞机不会进入 store（`core/src/store.rs:364-368`）；合法时先按同一时间戳写入初始状态并登记实时收包时刻，再追加 `Spawn` 事件（`core/src/store.rs:369-374`）。这一步没有响应，客户端无从得知 spawn 是否被接受。

## 状态、事件与遥测

三个投递方法共享同一套前置检查：先确认客户端未关闭，再确认可选时间戳有限，然后把操作压入队列（`core/src/transport/client.rs:555-566`、`:569-580`、`:583-594`）。它们都是同步方法，只负责入队。

| 方法 | 载荷 | 队列中的操作 |
| --- | --- | --- |
| `update_state(&self, state, timestamp)` | `pb::AircraftState` 与可选源时间戳 | `Operation::UpdateState`（`core/src/transport/client.rs:217`） |
| `create_event(&self, event_name, timestamp)` | 事件名与可选源时间戳 | `Operation::CreateEvent` |
| `publish_telemetry(&self, frame, timestamp)` | `pb::TelemetryFrame` 与可选源时间戳 | `Operation::PublishTelemetry` |

操作任务逐条把操作转换成对应消息再交给发送任务（`core/src/transport/client.rs:438-486`）。三条操作通道共用一个发送队列，状态、事件与遥测的相对顺序与提交顺序一致，心跳则插在它们之间。

服务端只接受 producer 会话上传的飞机事件，角色不符时回 `InvalidState` 与 `only producer sessions may upload aircraft events`（`core/src/transport/server.rs:504-513`）。通过角色校验的飞机事件不回 ACK，异常时才回错误响应，而客户端在握手之后不再读取套接字，这些错误响应不会被调用方看到（`core/src/transport/server.rs:526-547`）。

store 按命令类型分派（`core/src/store.rs:359-388`）：状态更新写入状态序列并刷新实时收包时刻；自定义事件写入事件序列；遥测帧先按 spawn 声明的 schema 校验，失败只记 warn 并丢弃；despawn 写入 `Despawn` 事件并清除实时收包时刻。源时间戳非有限的消息在分派前就被丢弃（`core/src/store.rs:333-337`）。

## 心跳与会话过期

心跳任务按固定间隔把 `Heartbeat` 放入发送队列，载荷是自增序号与 client UUID（`core/src/transport/client.rs:59-67`、`:497-528`）。间隔取自构造参数，并按 `Duration::from_secs_f64(heartbeat_interval_secs.max(0.1))` 取下限 0.1 秒（`core/src/transport/client.rs:498`）。定时器首个 tick 立即完成，序号从 0 自增后才发送，因此第一条心跳的序号是 1（`core/src/transport/client.rs:501-517`）。

客户端不等心跳 ACK，也不读取它（`core/src/transport/client.rs:510-518`）。服务端对每条 `Heartbeat` 都回 ACK，且不检查该地址是否已有会话（`core/src/transport/server.rs:468-470`），所以热线心跳无法确认自己是否还登记在会话表里。

会话是否活着由服务端按单调时钟判断。`is_expired` 比较 `last_seen_at.elapsed()` 与超时（`core/src/transport/server.rs:58-67`），而 `last_seen_at` 只在该地址已有会话时被 `touch_addr` 刷新（`core/src/transport/server.rs:163-169`）。接收循环每次读包前先执行 `cleanup_expired`（`core/src/transport/server.rs:313-318`），有效超时取配置的 `heartbeat_timeout_secs.max(1)`，即至少 1 秒（`core/src/transport/server.rs:308`）。被清理的地址不再接受飞机事件，客户端要重新握手才能恢复上报。

## 后台任务与队列

客户端的构造过程启动三个后台任务，并各自保留 JoinHandle 供 `close` 收尾（`core/src/transport/client.rs:295-306`）。

| 任务 | 职责 | 退出条件 |
| --- | --- | --- |
| sender | 消费出站队列并发送数据报 | 发送失败或收到 `Outbound::Shutdown`（`core/src/transport/client.rs:401-421`） |
| operation | 先发 spawn，再把用户操作转成消息 | 收到 `Operation::Stop`（`core/src/transport/client.rs:424-494`） |
| heartbeat | 定时投递心跳 | watch 通道置位（`core/src/transport/client.rs:497-528`） |

出站队列与操作队列都是无界通道（`core/src/transport/client.rs:397-399`）。发送任务一旦发送失败就退出并关闭套接字，此后入队的消息只得到一条 warn，不会阻塞调用方（`core/src/transport/client.rs:406-420`、`:684-693`）。

## 关闭与 despawn

`despawn(&mut self, reason, timestamp)` 结束单架飞机的生命周期：已 despawn 时直接返回 `Ok(())`，否则入队 `Despawn` 并置位（`core/src/transport/client.rs:597-610`）。despawn 之后会话与心跳仍在，客户端仍可继续发送消息，服务端也仍会为该飞机追加新的状态样本。

`close(&mut self)` 收紧整个客户端：已关闭时直接返回（`core/src/transport/client.rs:613-616`）；尚未 despawn 时补发一条原因为 `client_close` 的 `Despawn`（`core/src/transport/client.rs:626-634`）；随后通知心跳任务停止、向操作任务发送 `Operation::Stop` 并置空操作队列（`core/src/transport/client.rs:636-647`）；最后依次等待心跳、操作与发送三个任务结束，再置 `closed`（`core/src/transport/client.rs:649-660`）。

关闭后所有公开方法都以 `ClientChannelClosed("client is closed")` 失败（`core/src/transport/client.rs:663-668`）。Python 绑定把这层错误统一转成 `ConnectionError`（`bindings/python/src/client.rs:184`、`:198`、`:242`、`:258`），并在内部用同样的文案做前置检查（`bindings/python/src/client.rs:341-345`）。close 不会立刻删除服务端会话，服务端仍要等心跳超时才会清掉它。

## 绑定侧的封装

Python 绑定把这条会话包成 `FlyRulerClient`：构造函数先校验心跳周期是有限正数，否则直接抛 `ValueError`，不进入网络流程（`bindings/python/src/fly_ruler_proto_python/client.py:124-127`），随后调 `AircraftClient::connect_with_telemetry_at`（`bindings/python/src/client.rs:130`）。握手、spawn 与三个后台任务都在构造函数返回前完成。

`close()` 用内部标志位实现幂等，重复调用不再动作（`bindings/python/src/fly_ruler_proto_python/client.py:225-229`）；上下文管理器退出时调用同一个 `close()`，对象回收时再兜底调用一次并吞掉异常（`bindings/python/src/fly_ruler_proto_python/client.py:239-259`）。`despawn` 只影响飞机，不关闭连接（`bindings/python/src/fly_ruler_proto_python/client.py:210-223`）。

## 可靠性边界

UDP 通路没有重传与确认，除了握手那 1 秒的等待之外，状态、事件与遥测都是尽力而为：数据报丢失、乱序或超长被丢弃时客户端收不到任何通知，服务端也不会要求补发。唯一的顺序保证来自单条发送队列，跨进程不成立。握手之后客户端不再读取套接字，服务端的错误响应与心跳 ACK 都止步于内核缓冲区。

服务端会忽略客户端发来的 `Response` 与 `ServerPush` 信封，未握手地址的数据报也不会建立会话（`core/src/transport/server.rs:515-521`）。需要可靠、可重放的推送时应改用游标流：它按序号分片、按事件重传并逐条确认，回放侧的操作见 `docs/guide/06-sessions.md`。

## 相关页面

- [架构总览](../01-architecture.md)：core 分层与数据流
- [Python 绑定](../02-python-binding.md)：关闭语义如何映射到 Python 异常
- [内核分层与并发](kernel-concurrency.md)：会话表、过期清理与接收队列的位置
- [接口参考](../api.md)：传输层与绑定的公开名字
