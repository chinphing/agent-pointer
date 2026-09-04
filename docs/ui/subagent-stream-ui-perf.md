# 多子 Agent 流式 UI 性能

同时启动大量子 Agent（例如 100 个、并发 10）时，前端主线程容易被打满：
滚动空白、点击无响应；全部结束后又恢复流畅。根因是 **流式事件 → Pinia 写入 → 大量 `SubAgentFrame` 重算** 的扇出，而不是后端并发本身。

## 约定

1. **共享 flush**：`reasoningDeltaBatch` 对同一类 delta 使用 **一个** 定时器，到期后一次性写出所有 pending key，避免 10 路流各自触发多次 Vue tick。
2. **高压拉长间隔**：pending key ≥ `HIGH_PRESSURE_STREAM_KEYS`（6）时，batch 至少 `HIGH_PRESSURE_BATCH_MS`（**500ms**）。单路时仍用各类原间隔（content 50ms 等，与既有主气泡优化一致）。
3. **`assistant_json_partial` 必须批处理**：子 Agent 正文走 JSON 局部字段（thoughts / toolName / responseText），不得逐 token 写 store；latest-wins 合并；默认 **`ASSISTANT_JSON_PARTIAL_BATCH_MS` = 200ms**（收缩态 live 行对延迟不敏感）。
4. **`raw_content_delta`**：关闭「原始内容」时，若目标已是 streaming，不要再 dirty 状态字段。
5. **`SubAgentFrame`**：
   - 父级用 `v-memo`：非 running 只跟 status / 展开 / 看板版本；running 才带 live fingerprint。
   - 排队且收缩的帧跳过全量 `messages` 扫描。
6. **P0 — trace 索引 + 增量 live fingerprint**（已实现）：
   - `scopedTraceIndex.ts`：按 `anchorMessageId + traceId` 分桶，scoped 行注册一次；内容/工具 in-place 变更无需重扫全量 `messages`。
   - `scopedTraceCache.ts`：Pinia 侧 `liveSignals`；流式写入经 `notifyScopedStreamWrite` 只 touch 对应 trace。
   - `AssistantModelMessage`：`v-memo` 读 `subAgentLiveSignals[convId][traceId]`，不再 O(running × messages) 全表 filter。
   - `SubAgentFrame`：`scopedMessagesForTraceCached` 走索引 bucket，不再每帧多次 `messages.filter`。
7. **虚拟列表空白**：子帧高度在流式中频繁变高时，TanStack 会反复 measure；减少无效重渲染即可明显缓解。进一步可对单条消息内上百个子帧做虚拟化（尚未做）。

实现：`src/lib/reasoningDeltaBatch.ts`、`src/lib/scopedTraceIndex.ts`、`src/stores/chat/scopedTraceCache.ts`、`messageHandlers.ts`、`AssistantModelMessage.vue`、`SubAgentFrame.vue`。

**与既有流式优化的边界**：`useThrottledMarkdown`（100ms / 长文 250ms）与 content delta 默认 50ms **不改**；本页只约束 fan-out / json_partial 路径。
