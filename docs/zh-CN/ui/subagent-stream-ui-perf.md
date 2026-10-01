# 多子 Agent 流式 UI 性能

同时启动大量子 Agent（例如 100 个、并发 10）时，前端主线程容易被打满：
滚动空白、点击无响应；全部结束后又恢复流畅。根因是 **流式事件 → Pinia 写入 → 大量 `SubAgentFrame` 重算** 的扇出，而不是后端并发本身。

## 约定

1. **共享 flush**：`reasoningDeltaBatch` 对同一类 delta 使用 **一个** 定时器，到期后一次性写出所有 pending key，避免 10 路流各自触发多次 Vue tick。
2. **高压拉长间隔**：pending key ≥ `HIGH_PRESSURE_STREAM_KEYS`（6）时，batch 至少 `HIGH_PRESSURE_BATCH_MS`（**500ms**）。单路时仍用各类原间隔（content 50ms 等，与既有主气泡优化一致）。
3. **`assistant_json_partial` 必须批处理**：子 Agent 正文走 JSON 局部字段（thoughts / toolName / responseText），不得逐 token 写 store；latest-wins 合并；默认 **`ASSISTANT_JSON_PARTIAL_BATCH_MS` = 200ms**（收缩态 live 行对延迟不敏感）。
4. **`raw_content_delta`**：关闭「原始内容」时，若目标已是 streaming，不要再 dirty 状态字段。点数跟 `thoughts` / `reasoning`，不跟 raw。
5. **`SubAgentFrame`**：
   - 父级用 `v-memo`：非 running 只跟 status / 展开 / 看板版本；running 才带 live fingerprint。
   - 排队且收缩的帧跳过全量 `messages` 扫描。
   - live fingerprint 必须带上当前工具的 `displaySummary` / `arguments` 长度。只跟 status 时，start 种类名画上之后，后面到的 `label`/`goal` 不会重画。
6. **P0 — trace 索引 + 增量 live fingerprint**（已实现，**v1**）：
   - `scopedTraceIndex.ts`：按 `anchorMessageId + traceId` 分桶，scoped 行注册一次；内容/工具 in-place 变更无需重扫全量 `messages`。
   - `scopedTraceCache.ts`：Pinia 侧 `liveSignals`；流式写入经 `notifyScopedStreamWrite` 只 touch 对应 trace。
   - `AssistantModelMessage`：`v-memo` 读 `subAgentLiveSignals[convId][traceId]`，不再 O(running × messages) 全表 filter。
   - `SubAgentFrame`：`scopedMessagesForTraceCached` 走索引 bucket，不再每帧多次 `messages.filter`。
   - **v2（落地中）**：`ConversationScopedStore` + `scopedByInstance`（按 SpawnId / `agentInstanceId`）；hydrate 与 stream 不再把 scoped 写入 `conv.messages`。见 [conversation-scoped-messages-v2.md](../design/conversation-scoped-messages-v2.md)。
7. **虚拟列表空白**：子帧高度在流式中频繁变高时，TanStack 会反复 measure；减少无效重渲染即可明显缓解。进一步可对单条消息内上百个子帧做虚拟化（尚未做）。
8. **按 spawn 订阅 live signal**（已实现）：`publishLive` 只改该 spawn 的 key，不替换整份 `liveSignals`。`scopedMessagesForTraceCached` 只 `void` 本路 fingerprint。文件变更条只订阅当前窗口的 spawn id + membership（增删 spawn 才变），不 `void` 整表。`touchRow` 不再 bump 全局 `version`。
9. **spawn 启动 / `agent_trace` 写回**（已实现）：`sub_message_start` 的空壳回收只扫 **本 spawn**；lead `MessageStart` 只扫 `conv.messages`。`agent_trace` 落库仅终态（completed/failed/cancelled）；running 只走 SSE。
10. **P1-lite — 终态折叠 idle 卸载**（已实现）：
   - **仅** `completed` / `failed` / `cancelled` 且 `userExpanded=false`；**running 永不卸载**。
   - UI 分页 hydrate **默认不加载** scoped 子 Agent 行（`context_included=0`）；Stub **默认仍显示统计行**，读 `agentTrace.summaryLine`（终态时按 scoped / session.stats **重算覆盖**写入）。缺摘要时补一次 lazy load 再写回，然后 evict。
   - 用户点过程行展开 → `loadScopedSubMessagesForTrace` 写入 `ConversationScopedStore`（不 merge 进 `conv.messages`）。无 `summaryLine` / 尚无内层工具时仍显示「过程」并可点。
   - Stub 时 `evictInstance` 释放该 spawn 的 scoped 堆。
   - 会话加载时已是终态 → 立刻 Stub；同会话内终态折叠 → **60s** 后再 Stub。

实现：`src/lib/subAgentFrameMount.ts`、`useTerminalSubAgentStub.ts`、`SubAgentFrameHost.vue`、`SubAgentFrameStub.vue`、`src/lib/conversationScoped/store.ts`、`closeAbandonedEmptyAssistantShells`、`should_persist_anchor_agent_trace`。

11. **`sendChat` / `persistAppend`**：dispatch 只克隆未落盘 **lead**；`persistAppend` 用 `listUnpersistedRows`，禁止 `listRows` 摊平后再 clone。增量空不再全量 fallback。
12. **`agentTrace` 写回**：终态 coalesce ~400ms 后 `json_set` 只改 `payload.agentTrace`；内存仍即时更新。前端 `ensureSubTrace` 不替换已有数组。
13. **`is_scoped` 列（schema v26）**：UI 页 `WHERE is_scoped = 0` + `idx_messages_conv_lead_pos`。不用 `context_included` 当 UI 过滤。
14. **Host 按 spawn 订阅**：父气泡不读整表 `liveSignals`。每路 Host 自己 `getSubAgentLiveSignal`；有 `summaryLine` 的终态 Stub 不再拉 spawn 行。
15. **`findRow` O(1)**：`rowById`。压缩归属用切点那一行，不 `scopedRowsForAnchors` 拼全部 spawn。
16. **展开 lazy load**：有 `agent_instance_id` 时 `WHERE is_scoped = 1 AND agent_instance_id = ?`，不再对该路 `json_extract`。
