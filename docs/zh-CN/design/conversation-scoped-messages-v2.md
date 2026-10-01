# 会话 Scoped 消息存储 v2：双容器 + SpawnId

> **状态**：A–E 前端切过已落地；F 进行中；G 未开始。  
> **已知前端漏洞先记、后改**（见 §14），不要在修 F/G 时顺手改那些路径。  
> **关联**：[subagent-goal-context-and-nesting.md](subagent-goal-context-and-nesting.md)、[async-subagent-and-terminal.md](async-subagent-and-terminal.md)、[../ui/subagent-stream-ui-perf.md](../ui/subagent-stream-ui-perf.md)。

---

## 1. 背景与问题

### 1.1 现状

前端会话内存中，lead timeline 与 scoped 子 Agent 过程行混在同一 `conv.messages: ChatMessage[]` 中，通过 `anchorMessageId` 标记 scoped 行。

为性能（P0），又引入 **派生索引** `ScopedTraceIndex` / `scopedTraceCache`：从 `conv.messages` rebuild 或增量 touch，按 `anchor + traceId` 分桶，供 `SubAgentFrame` O(1) 读取与 `v-memo` live fingerprint。

分页 hydrate 已支持 **默认不加载 scoped**（`includeScopedSubMessages: false`）；展开时 **`loadScopedSubMessagesForTrace`** 按需拉取。Stub 优化进一步减少 DOM，但 **scoped 堆内存** 在同会话流式写入后仍留在 `conv.messages`。

### 1.2 痛点

| 痛点 | 说明 |
|------|------|
| **双轨 ownership** | scoped 既在 `messages`，又在 index rebuild；谁才是源不清晰 |
| **traceId 语义混乱** | `task:agent` 与 `task:instance:agent` 两套格式；coder 续跑共用 traceId、靠 instanceId 分 scoped |
| **Stub 无法释放堆** | DOM 卸载 ≠ `conv.messages` 去掉 scoped |
| **索引冗余** | `scopedByInstance` 落地后，`ScopedTraceIndex.buckets` 与 `agentInstanceId` filter 重复 |
| **Lead 逻辑污染** | layout/compression/trim 处处 `isScopedSubMessage` 防御 |

### 1.3 目标

1. **物理分离**：anchor timeline 与 scoped transcript 分容器存储。  
2. **SpawnId 统一**：一次 `run_subagent` spawn = 一个 **`agentInstanceId`** = UI trace 主键 = scoped 桶 key。  
3. **单一写入口**：`ConversationScopedStore` 替代 `ScopedTraceIndex` + merge 进 `messages`。  
4. **生命周期一等公民**：stub evict、trim evict、会话 evict 直接删 bucket。  
5. **长远可迭代**：新功能只扩展 store API，不再 patch flat array + rebuild index。

### 1.4 非目标（本阶段）

- SQLite 表结构拆分（可 Phase F 以后再做）。  
- 改动 LLM 侧 `strip_scoped_from_lead_history` 语义（后端已正确 exclude scoped）。  
- 子 Agent 虚拟列表（仍属 UI perf 独立项）。

---

## 2. 概念模型：SpawnId 取代 traceId 主键

### 2.1 两个 ID  today

| 字段 | 含义 | 典型形态 |
|------|------|----------|
| **traceId** (`AgentTrace.id`) | 逻辑任务 + worker 的字符串编码 | `task-1:coder` 或 `task-1:<uuid>:explore` |
| **agentInstanceId** | 每次 spawn 的 UUID | 全局唯一 |

后端规则（`sub_agent_trace_id`）：

- **coder / computer 等**：`traceId = taskId:agentId`（**不含** instanceId）。  
- **self / explore**：`traceId` **嵌入** instanceId（三段式）。

同一 `taskId` 续跑 coder 时，**共用 traceId**、**新 instanceId** → `ensureSubTrace` 合并为一条 UI trace，scoped 靠 instance 区分。

### 2.2 v2 约定

```ts
/** 一次 sub-agent spawn 的唯一标识；与 agentInstanceId 同义 */
type SpawnId = string
```

| 项目 | v2 |
|------|-----|
| **AgentTrace.id** | **`SpawnId`（= agentInstanceId）** |
| **scopedByInstance key** | **`SpawnId`** |
| **taskId / agentId** | **显式字段**，不再从 traceId 字符串 parse |
| **traceId 字符串** | 过渡期 SSE/DB 兼容；**不作主键** |
| **一次 spawn 一条 AgentTrace** | 续跑 / 并行不再共用 `task-1:coder` 一行 |

逻辑任务关联用 **`taskId`**（task board、dependsOn、续跑），不用共用 trace id。

### 2.3 AgentTrace 形状（前端 / 持久化）

```ts
interface AgentTrace {
  /** v2：= agentInstanceId */
  id: SpawnId
  agentInstanceId: SpawnId

  taskId: string
  agentId: string          // explore | coder | self | ...

  name: string
  role: string
  status: string
  depth?: number
  collapsed: boolean
  userExpanded?: boolean
  summaryLine?: string     // stub 摘要

  parentToolCallId?: string
  anchorMessageId?: string
  computerTarget?: ComputerOperationTarget

  /** 只读兼容旧数据 */
  session?: SubAgentSessionUi
}
```

**废弃**（读兼容、写不再生成）：

- `subTaskIdFromTraceId` / `subAgentIdFromTraceId` / `agentInstanceIdFromTraceId` 作为主路径  
- `scopedTraceBucketKey(anchor, traceId)`

---

## 3. 数据模型

### 3.1 Conversation

```ts
interface Conversation {
  id: string
  // ... 现有 meta（title, workspaceRoot, agentMode, ...）

  /** Lead timeline：user / lead assistant / host tool / 压缩占位；永不含 scoped */
  messages: ChatMessage[]

  /** Scoped 源数据；key = SpawnId（agentInstanceId） */
  scopedByInstance: Map<SpawnId, ScopedInstanceTranscript>
}
```

### 3.2 ScopedInstanceTranscript

```ts
interface ScopedInstanceTranscript {
  instanceId: SpawnId
  anchorMessageId: string
  taskId: string
  agentId: string

  /** 有序过程行（assistant / tool）；按 createdAt 或 seq */
  rows: ChatMessage[]

  loadState: 'streaming' | 'loaded' | 'evicted'

  /** SubAgentFrame v-memo；终态 stub 后可清空 rows 但可保留 fingerprint 空串 */
  liveFingerprint: string
}
```

`rows` 内 `ChatMessage` 仍带 `anchorMessageId` / `agentInstanceId`（与 DB payload 对齐），但 **不再出现在 `conv.messages`**。

### 3.3 辅助索引（会话级，内聚在 Store）

```ts
interface ConversationScopedIndexes {
  /** stream 仅带 scopedMessageId 时 O(1) 定位 */
  messageIdToInstance: Map<string, SpawnId>

  /** trim / 按 anchor 批量 evict */
  instancesByAnchor: Map<string, Set<SpawnId>>
}
```

**删除**：`ScopedTraceIndex`（`buckets` + `messageKeys` + rebuild）。

---

## 4. ConversationScopedStore

**路径建议**：`src/lib/conversationScoped/`（或 `src/stores/chat/scopedStore.ts`）。

Pinia `chat.ts` 仅保留薄封装；**禁止**在外部直接 mutate `scopedByInstance`。

### 4.1 读 API

| 方法 | 用途 |
|------|------|
| `getRows(spawnId)` | SubAgentFrame 渲染 |
| `getRowsForTrace(trace: AgentTrace)` | `trace.id` → rows |
| `getLiveSignal(spawnId)` | `v-memo` |
| `findRow(messageId)` | findMessage / resolveToolCall |
| `collectFileMutationsForTurn(anchorIds)` | lastTurnFileChanges |
| `hasLoaded(spawnId)` | lazy load 短路 |

### 4.2 写 API

| 方法 | 用途 |
|------|------|
| `ensureInstance(meta, firstRow?)` | `sub_message_start` |
| `mutateRow(messageId, patch)` | 流式 delta / tool / message_end |
| `assignLoaded(spawnId, rows)` | lazy load 结果 |
| `evictInstance(spawnId, reason)` | stub / manual |
| `evictForAnchor(anchorMessageId)` | trim 联动 |
| `clearConversation(convId)` | idle evict |

每次 `mutateRow` 后更新 `liveFingerprint`（逻辑自 `computeSubAgentLiveFingerprint` 迁入）。

### 4.3 Vue 响应式

- `scopedByInstance` 使用 `shallowRef<Map<...>>`。  
- 结构变更（ensure / evict / assign）：**克隆 Map 再赋值**。  
- row 内容 in-place mutate（tool result、content）：仅 fingerprint 变化时 bump signal，与 P0 策略一致。

---

## 5. 各路径行为

### 5.1 Hydrate（分页）

```
loadConversationMessagesPage(includeScopedSubMessages: false)
  → conv.messages = anchors only
  → scoped store 空或保留同会话 in-flight 实例
  → normalizeSubAgentTraces：读 lead.agentTrace + summaryLine
```

**无** `rebuildScopedTraceIndex`。

### 5.2 Lazy load（展开 / 运行中 full frame）

```
需要 rows 且 loadState !== loaded|streaming
  → loadScopedSubMessagesForTrace({ agentInstanceId, ... })  // 过渡仍可用 anchor+trace
  → store.assignLoaded(spawnId, rows)
```

### 5.3 Stream

```
sub_message_start → ensureInstance + ensureSubTrace(lead)   // trace.id = spawnId
tool_call_* / deltas → mutateRow(scopedMessageId)
agent_step 终态 → persist summaryLine 到 AgentTrace；可选 evict 调度
```

`resolveStreamWriteMessage`：

- 有 `scopedMessageId` → `store.findRow` / `mutateRow`  
- 无 → lead `anchorMsg`

### 5.4 Stub（P1-lite 延续）

```
终态 + collapsed + idle/stubImmediate
  → SubAgentFrameStub（读 trace.summaryLine）
  → store.evictInstance(spawnId, 'stub')
用户展开
  → userExpanded = true → lazy load → assignLoaded
```

### 5.5 Trim / Evict

```
trimConversationHistory
  → slice conv.messages
  → 对被移除的 anchorMessageId：evictForAnchor

evictConversation (idle 120min)
  → messages = [] ; scopedByInstance.clear()
```

### 5.6 MessageList / Compression

- **删除** `messageListLayout` 中 `isScopedSubMessage → skip`（scoped 不在 array）。  
- **compressionLayout** 仅扫 `messages`。  
- **subAgentFrameOwnsCompression** 改用 `store.getRows(spawnId)`。

### 5.7 rehydrateAgentTracesFromScopedMessages

- **默认删除**；hydrate 不加载 scoped 时无数据源。  
- Legacy：首次 lazy load 后可选 backfill `AgentTrace` 字段，不做全表 filter。

---

## 6. 后端与 API（分阶段）

### Phase A–E：前端-only（SQLite 不变）

| 方向 | 行为 |
|------|------|
| Anchor page | **lead 行**（无 `anchorMessageId`）。不能用 `context_included = 1`：该列同时丢掉压缩软排除的主时间线 |
| Scoped lazy | 已有 `load_scoped_sub_messages_for_trace` |
| Persist | 仍由 Rust append flat；前端 store 与 DB 独立 |

Lazy load 查询过渡参数：

```ts
{ anchorMessageId, traceId?, agentInstanceId }  // instance 必填优先
```

### Phase F：API 增量（已落地，无表拆分）

- Lazy load：`agentInstanceId` 有则按列查；否则 `anchor + traceId`。  
- `MessagePage.scoped`：`includeScopedSubMessages=true` 时按 SpawnId 分桶；`messages` 只留 lead。默认 hydrate 不填 `scoped`。  
- 不新增 `scoped_messages` 表、无迁移脚本。

### Phase G（已落地）：SSE / `AgentTrace.id` 以 SpawnId 为主

```ts
{ kind: 'tool_call_start', agentInstanceId: SpawnId, scopedMessageId: string }
// traceId deprecated，保留一版兼容
```

Rust `sub_agent_trace_id` 改为 mint `id = instance_id`。**等 §14 修完再做。**

---

## 7. 删除与替换清单

| 删除 / 废弃 | 替代 |
|-------------|------|
| `src/lib/scopedTraceIndex.ts` | `conversationScoped/store.ts` |
| `src/stores/chat/scopedTraceCache.ts` | 同上 |
| `rebuildScopedTraceCache` | 无 rebuild |
| `mergeScopedMessagesIntoConv` | `assignLoaded` |
| `ensureScopedChildMessage` → push messages | `ensureInstance` |
| `scopedMessagesForTrace(messages, ...)` | `getRows(spawnId)` |
| `scopedMessagesForTraceCached` | store 薄封装 |
| `findScopedMessage(conv, id)` | `store.findRow(id)` |
| lead 上 `isScopedSubMessage` 防御（layout 等） | scoped 不在 messages |

**保留**（adapter / 类型守卫）：

- `isScopedSubMessage`：DB payload 判定、lazy load 行校验  
- `load_scoped_sub_messages_for_trace`（Rust）

---

## 8. 与现有文档关系

| 文档 | 变更 |
|------|------|
| [subagent-stream-ui-perf.md](../ui/subagent-stream-ui-perf.md) | §6 P0 index 标注为 **v1**；v2 见本文 §4 |
| [subagent-goal-context-and-nesting.md](subagent-goal-context-and-nesting.md) | §4.1 linkage：`traceId` 主键改为 SpawnId + 显式 taskId/agentId |
| [session-search-scope-extension.md](session-search-scope-extension.md) | 已按 instance 检索；与 SpawnId 一致 |

---

## 9. 实现计划

### Phase A — 基础设施（~2d）

- [x] A1 `ScopedInstanceTranscript` / store 分桶（未塞进 Pinia `Conversation` 对象；单例 store）  
- [x] A2 `ConversationScopedStore` + indexes + 单测  
- [x] A3 `AgentTrace.taskId` / `agentId` 显式字段；**`id` = SpawnId**（Phase G）  
- [x] A4 Feature flag **跳过**：直接切 store，无双写回滚

### Phase B — 写入路径（~2–3d）

- [x] B1 `subMessageHandlers` → `ensureInstance`  
- [x] B2 `messageHandlers` / `toolHandlers` → store `findRow` / in-place mutate  
- [x] B3 `resolveStreamWriteMessage` / `findMessage` → store（仍 fallback `conv.messages`）  
- [x] B4 Rust emit：`AgentTrace.id = agent_instance_id`（Phase G）

### Phase C — 读取路径（~1–2d）

- [x] C1 `SubAgentFrame` / `SubAgentFrameHost` → store `getRows`  
- [x] C2 `agentHandlers` summaryLine / stats  
- [x] C3 已删除 `scopedTraceCache`

### Phase D — 生命周期（~1–2d）

- [x] D1 stub → `evictInstance`（嵌套级联见 §14-4，后改）  
- [x] D2 `ensureScopedMessagesForTrace` → `assignLoaded`  
- [x] D3 trim / idle / delete / logout 联动  
- [x] D4 `lastTurnFileChanges` / compression / computer compact 读 store

### Phase E — 清理（~1–2d）

- [x] E1 删除 `scopedTraceIndex.ts` / `scopedTraceCache.ts`  
- [x] E2 移除 messages 上 scoped 写入路径  
- [ ] E3 简化 `messageListLayout` / `compressionLayout` 的 `isScopedSubMessage` 防御（hydrate 已不含 scoped；保留守卫无功能害）  
- [x] E4 更新 [subagent-stream-ui-perf.md](../ui/subagent-stream-ui-perf.md)  
- [x] E5 无 flag

### Phase F — 后端（进行中）

- [x] F1 Lazy load：**`agent_instance_id` 优先**；无 instance 时仍走 `anchor + traceId`  
- [x] F2 `MessagePage.scoped`：`includeScopedSubMessages=true` 时 scoped 行从 `messages` 拆到 `scoped[spawnId]`；默认 hydrate 仍只返回 anchors  
- [x] F3 SSE / persist 已带 `agentInstanceId`；Rust `AgentTrace` 补显式 `taskId` / `agentId`（**不改 `id`**）  
- [x] F4 **无表拆分、无迁移脚本**：旧行靠 `anchor+traceId` 读兼容；`agent_instance_id` 列已存在

### Phase G — SSE / UI 主键改为 SpawnId

- [x] G1 `AgentTrace.id = agent_instance_id`（`ChatMessage.trace_id` 仍用 legacy `sub_agent_trace_id`）  
- [x] G2 `ensureSubTrace` / `findSubTrace` 按 SpawnId 合并；同 `taskId` 多次 spawn = 多行  
- [x] G3 流事件仍带 legacy `traceId`；UI 用显式 `taskId`/`agentId`，`traceLookupId` 兼容任务板

---

## 10. 测试计划

| 类别 | 场景 |
|------|------|
| **Store 单测** | ensure / mutate / evict / findRow / fingerprint |
| **Stream 集成** | sub_message_start → tool_call → terminal；scoped 不在 messages |
| **Hydrate** | 默认无 scoped；stub 用 summaryLine |
| **Expand** | lazy load → rows 出现在 store |
| **Stub evict** | 60s / hydrate immediate；内存 map 无 bucket |
| **Trim** | 旧 anchor trim 后 instance evict |
| **并行** | 10× explore；10 个 SpawnId 互不干扰 |
| **续跑** | 同 taskId 新 instance → 新 AgentTrace 行 |
| **Legacy** | 旧 traceId 格式会话只读 + lazy load 正常 |
| **App + Web** | Tauri / web 同一 store 逻辑 |

---

## 11. 风险与缓解

| 风险 | 缓解 |
|------|------|
| Map 响应式丢失 | 统一 store commit；单测 + 流式手动 QA |
| 旧会话无 instanceId | lazy load 从 row 回填；trace 解析 fallback |
| 双写过渡期不一致 | Feature flag；CI 双路径测试 |
| task board 仍用 traceId | 改为 `spawnId` 或显式 `taskId` lookup |
| 后端 SSE 仍发 traceId | 前端 map：traceId → spawnId 过渡表（仅旧流） |

---

## 12. 实现进度

| Phase | 状态 | 备注 |
|-------|------|------|
| A–E | 完成 | 前端双容器切过；无 feature flag。遗留 E3 布局简化、B4/G 主键对齐 |
| F | 完成（本切片） | instance 优先 lazy load；`MessagePage.scoped`；`AgentTrace.taskId/agentId` 落 Rust。无 SQLite 侧表 |
| G | 完成 | `AgentTrace.id = SpawnId`；legacy `traceId` 仅兼容 SQLite / SSE |
| §14 漏洞 | K1–K11 已修 | K11：展示过滤 / store 查找不再要求 `row.traceId === AgentTrace.id` |

---

## 13. 摘要

- **存储**：`messages` = anchor timeline；`scopedByInstance` = scoped 唯一源。  
- **主键**：**SpawnId = agentInstanceId = AgentTrace.id**；task/agent 用显式字段。  
- **索引**：删除 `ScopedTraceIndex`；Store + `messageIdToInstance` 即可。  
- **加载**：hydrate 与 lazy load **已分离**；v2 仅改 lazy 结果写入 store。  
- **内存**：stub / trim / evict 删除 bucket，不再留 scoped 在 flat array。

---

## 14. 已知问题

前端 A–E 切过审查。主路径（一层 `run_subagent` + hydrate stub + 展开 lazy load）能跑。

| ID | 严重度 | 状态 | 说明 |
|----|--------|------|------|
| **K1** | 高 | **已修** | `sub_message_start` 用 `findMessage`（含 store）找嵌套 anchor |
| **K2** | 高 | **已修** | running 不 lazy load；`assignLoaded` 只补缺 id，不替换 streaming 对象 |
| **K3** | 中高 | **已修（缓解）** | legacy `traceId` 回退取**最后一次** spawn；仍建议 UI 带 `agentInstanceId` |
| **K4** | 中 | **已修** | `evictInstance` 按子行 id 递归 evict 嵌套 spawn |
| **K5** | 中 | **已修** | 缺 spawn id：`warn + return`，不 throw |
| **K6** | 中 | **已修** | 终态 / stub evict 把 tool id 记在 `AgentTrace.searchToolCallIds`，搜索可钉住再 hydrate |
| **K7** | 低 | **已修（Phase G）** | 同 task 多次 spawn = 多行 `AgentTrace` |
| **K8** | 低 | **已修** | `sendChat` 带 store 行；`prepare_lead_history` 先 `append_missing` 再丢掉 scoped |
| **K9** | 低 | **已修** | evict 删除 `bySpawn` / liveSignals key |
| **K10** | 低 | **已修** | `touchRow` / `touchLookup` 只改该 spawn 的 live signal，不 bump 全局 `version` |
| **K11** | 高 | **已修** | `scopedMessagesForTrace` 有 instance 时只比 instance；无 instance 时 `row.traceId` 或 `row.agentInstanceId` 均可。`resolveSpawnId` 把无冒号 id 当 SpawnId，避免 `anchor\\0uuid`。`getTranscript` 回退也比 `instanceId`。压缩归属在 UUID id 上不再误用 `subAgentIdFromTraceId` |

**有意保留、不算漏洞**

- hydrate 默认不拉 scoped；`takeScopedFromMessages` 拆走误入分页的行  
- 一层 spawn：`sub_message_start` → store → `findMessage`  
- `persistAppend` 只克隆未落盘 lead + `listUnpersistedRows`；`sendChat` 不摊 scoped 
- 终态 stub 用 `summaryLine`，不依赖 evict 后的 rows
- 无摘要时过程行仍显示「过程」，展开必走 lazy load（不要用「没有内层工具」挡点击）
