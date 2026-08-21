# Web 端聊天流弱网对账（SSE gap）

## 问题

网页端通过 `GET /api/chat/:id/stream`（SSE）接收 `StreamEvent`。

路由：

| 订阅 | 收到的事件 |
| --- | --- |
| `global` | 无会话 id 的进程级事件（控制台 PTY、通道配对），以及 **当前登录用户自己的** 会话事件 |
| 具体 `conversationId` | **仅** 该会话（默认拒绝，不再把缺字段的 delta 推给所有连接） |

事件只在广播环里存活：

- 客户端慢 / 网络弱时，`tokio::broadcast` 会 **Lagged**，已发出的帧（含 `delta` / `done`）被丢掉且**不重放**
- SSE 断线重连后同样拿不到断线期间的事件

表现：

1. 服务端已落盘完整回复，界面要刷新才看到
2. UI 一直停在「执行中」（`generating` 未清，或工具 / agentTrace 仍 `running`）
3. 弱网丢 `done` 后当前会话仍显示执行中
4. **新会话首条**：`onStream` 未等 SSE 挂上就 `POST /api/chat`，广播零订阅丢帧，刷新后才看到回复

桌面端走 Tauri 事件通道，无此 SSE 环；本对账逻辑对桌面无害（`onGap` 为空操作，`waitForChatStreamReady` 立即返回）。

## 现行策略

| 层 | 行为 |
|----|------|
| Server `chat_stream` | 广播缓冲 4096；按会话信封投递（见上文）；`Lagged` 时打 warn，并向该 SSE 连接发 `event: resync` |
| Web `onStream` | **`chat.init` 一开始就发起**（与拉项目/会话列表并行），首次成功打开后才 resolve；断线期间 `waitForChatStreamReady` 为 false。收到 `resync`、流 body 结束、502/504/错误重连时调用 `onGap(reason)` |
| 发送闸门 | `dispatchChatTurn` 在 `POST /api/chat` 前 `await waitForChatStreamReady()`，避免新会话首条在零订阅时把帧丢掉 |
| 执行态对账 | `flags`：只对照 dispatcher 清/置 `generating`（online、visibility、boot） |
| **消息拉取** | **仅** SSE 断开类 reason 走全量 `catch_up`：`server_lagged` / `stream_ended*` / `stream_error` / `stream_gateway_error`（含兼容 `sse_gap`）。另外：`flags` 模式下若清掉「服务端已结束、UI 仍 generating」的会话，也会对该会话 `force` 水合（覆盖首条未挂上 SSE 的情况） |
| catch_up 水合 | `ensureMessagesLoaded({ force, silent })`，重新拉取最近回合窗口（非全量），不拨 hydrating UI |
| 合并 | 强制水合时保留 live streaming 标志，正文/工具取与 DB 更完整的一侧 |
| Done 提示音 | `generating` 或仍有 active turn timing 时播放 |

不再做 generating 期间的定时轮询；僵死执行态依赖 SSE gap / online / visibility。


## 相关文件

- `server/src/main.rs` — `chat_stream`
- `src/lib/web.ts` — `onStream` / `waitForChatStreamReady`
- `src/stores/chat.ts` — `dispatchChatTurn` / `syncRunStateFromDispatcherQueue` / `resyncAfterStreamGap`
