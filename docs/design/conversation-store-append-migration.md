# 对话存储与 Transcript 协调（P0–P2a）

> **状态**：**已实现**（2026-06）。后端 `run_chat` 由 `ConversationTranscript` 协调落库；前端 messages **append-only**；**已移除**对外 `replace` API 与 undo/retry 全量覆盖路径。

## 1. 背景与目标

| | 迁移前 | 当前 |
|--|--------|------|
| 主存储 | `conversations.json` → `conversations.db` | SQLite `conversations.db` 为磁盘真相源 |
| 写入路径 | 前端 debounce **全量** `save_conversations` | 后端 transcript 为主；前端 **meta + append** |
| 工具结果 | 独立 `role: tool` 行可能落在表尾（orphan） | 插入对应 assistant 之后；批末 `sync_ordered` |
| 会话统计 | 每条 upsert 后 `load_messages` 全量 reload | 内存 count/preview + `flush_conversation_meta` |
| 前端改历史 | `replace` DELETE 全表再 INSERT | **禁止**客户端全量覆盖（避免抹掉 tool 行） |

目标（Hermes 式 append 为主）已达成；Pointer 压缩仍为 **软标记 + append summary**，不是 replace。

## 2. 单一真相（三层）

| 层 | 内容 | 说明 |
|----|------|------|
| **磁盘 canonical** | `messages` 表中独立 `role: tool` 行 | 紧跟发出 `tool_calls` 的 assistant 行之后；稳定 id `tool_{tool_call_id}` |
| **UI 展示** | `assistant.toolCalls[].result/error` | 流式 `ToolCallStatus`；用户不看 `role: tool` 行 |
| **发 LLM（wire）** | `expand_tool_messages` 从有序 transcript 展开 | 孤立 tool 行 **warn + 跳过**（防御性，避免 OpenAI 400） |

**不要**在磁盘上依赖 inline `toolCalls[].result` 作为 tool 结果主存储；wire 展开以有序 `role: tool` 行为准。

## 3. 架构：`ConversationTranscript`

`run_chat` 期间每个活跃 `conversation_id` 注册一个会话级协调器，作为 **transcript 变更的唯一写入口**（与 `cancels` 同生命周期假设：同会话单活跃 run）。

```
crates/pointer-core/src/
├── conversation_transcript/     # run_chat 内 transcript 协调
│   ├── mod.rs                   # begin/end、record_tool_result、sync_ordered
│   ├── registry.rs              # conversation_id → active session
│   └── reconcile.rs             # orphan 清理、tool 插入锚点
├── conversation_store/
│   ├── persist.rs               # load、低层 SQL
│   ├── write.rs                 # append / sync_with_meta / flush_meta
│   └── mod.rs                   # ConversationStore
└── chat_service/
    ├── session.rs               # begin/end 挂点
    └── conversation_persist.rs  # 薄封装 → transcript
```

### 3.1 生命周期（`session.rs`）

```text
run_chat
  ├─ ConversationTranscriptSession::begin(history)
  │    ├─ reconcile_tool_messages（内存去掉 orphan tool）
  │    ├─ append_missing_messages（补前端已有、库中尚无的 id）
  │    ├─ 若 reconcile 改过顺序 → sync_messages_ordered_with_meta
  │    └─ register Registry
  ├─ run_chat_inner …
  └─ ConversationTranscriptSession::end(history)
       ├─ flush_transcript（若 transcript_dirty）
       └─ unregister Registry
```

`begin()` 失败时回退 `conversation_persist::append_missing`，**不**注册 Registry（应极少；打 warn）。

### 3.2 工具结果写入

- **内存**：`record_tool_result` 按 `tool_call_id` **向前**找含该 call 的 assistant（非「最后一条 assistant」）；插入 anchor 后 tool 块末尾；同 `tool_call_id` upsert。
- **磁盘**：不在每个工具结束时单独 upsert；**每轮 tool pass 结束** `flush_after_tool_pass` → 一次 `sync_messages_ordered_with_meta`。

### 3.3 Assistant / 注入 user 行

`commit_lead_assistant_turn`、`supervisor` 终稿、`json_tool_retries` 注入行：

- 有 Registry → `upsert_message_no_refresh` + `flush_conversation_meta`（增量 count/preview）
- 无 Registry → `upsert_no_refresh` + `COUNT(*)` + preview（打 warn）

## 4. 写入阶段定义（P0 / P1 / P2a）

```text
P0  run_chat transcript ──append/upsert/sync──► messages 表
P1  前端 UI           ──save_conversation_meta──► conversations 壳字段
P2a 压缩 / TaskBoard trim ──sync_ordered_with_meta──► 改 position/context，不 DELETE
（P2b replace 已废弃，见 §6）
```

### P0 挂点

| 时机 | 行为 |
|------|------|
| `begin` | reconcile + append_missing + 可选 sync |
| assistant commit | upsert 单行 + flush meta |
| tool pass 结束 | sync_ordered（批末） |
| `end` | 最终 flush |
| 压缩 / trim（仍在 run 内） | `conversation_transcript::sync_ordered` |

### P1 前端 meta-only

`persistMeta()` debounce 同步：`title`、`updatedAt`、`workspaceRoot`、`computerMonitorId`、`toolRoundsUsed*`、`skillIds` 等。

**不写** messages 全量的入口：`newConversation`、`deleteConversation`、`setConversationWorkspace`、`stream done`、workspace/monitor 更新等。

### P2a 有序 sync（不删行）

- 按 `history` 顺序 upsert `position` / `payload`
- 不 DELETE 库中多余 id（压缩只改 `context_state`、插入 summary）
- 统计：`sync_messages_ordered_with_meta(count, preview)`，**不** `load_messages`

## 5. Store API（Rust）

| API | 用途 |
|-----|------|
| `append_missing_messages` | 仅插入库中缺失的 message_id |
| `upsert_message_no_refresh` | 单行 upsert，不 reload 统计 |
| `flush_conversation_meta` | `UPDATE message_count, preview` |
| `sync_messages_ordered_with_meta` | P2a 有序 upsert + 轻量 meta |
| `message_count` / `stored_conversation_preview` | 轻量读 meta |
| `save_meta_all` | P1 壳字段 |
| `replace_messages` | **仅单元测试**；生产不调用 |

**已删除**：`sync_messages_ordered`（无 meta、内部 reload）、对外 `replace_conversation_messages`。

`refresh_conversation_stats`（全量 `load_messages`）已从生产路径移除。

## 6. 为何废弃 P2b `replace`

| 问题 | 说明 |
|------|------|
| 与 canonical 冲突 | 前端 messages **不含** `role: tool` 行；DELETE+INSERT 会抹掉磁盘 tool 行 |
| 曾触发 400 | orphan tool + wire 顺序错误（assistant 无 tool_calls 后跟 tool） |
| 产品无 undo/retry | UI 未暴露；无需客户端全量覆盖 |

### 6.1 前端落库策略（`chat.ts`）

| 场景 | 策略 |
|------|------|
| 正常对话 | 后端 `run_chat` 落库；前端 `persistMeta` |
| `sendChat` 网络失败 | `persistAppend`（`append_conversation_messages`） |
| 登录 / 额度错误（未发 chat） | **仅 UI**，不写 messages |
| 清除登录错误气泡 | **仅 UI** 过滤 |

### 6.2 对外 HTTP / Tauri

| 方法 | 路径 / command | 说明 |
|------|----------------|------|
| PUT | `/api/conversations/meta` | P1 meta |
| POST | `/api/conversations/:id/messages/append` | P0 追加缺失 id |
| — | `append_conversation_messages`（Tauri） | 同上 |

**不再提供** `PUT /messages` 全量 replace。

## 7. Orphan tool 与 wire 防御

**产生原因（历史）**：

- 后端 `push_tool_result` 曾在表尾 upsert tool 行
- 前端 `replace` / 仅 inline `toolCalls` 不同步 tool 行
- `append_missing` 只增不删 → user 后出现孤立 tool

**现状**：

- `begin`：`reconcile_tool_messages` 清理内存 orphan；必要时 sync 纠正 position
- `expand_tool_messages_for_openai_request`：wire 上孤立 tool **跳过 + warn**
- 插入：锚定含 `tool_call_id` 的 assistant

## 8. `message_count` / `preview` 规则

- `preview`：第一条非空 **user**，否则第一条 **assistant**（约 160 字）；不看 tool、不看最新消息。
- Transcript session 内存维护 count/preview；批末 / upsert 后 `flush_conversation_meta`。
- `append_missing` 后 count 取 `SELECT COUNT(*)`，preview 由调用方传入的 history 计算。

## 9. 与 Hermes 差异

- Hermes：压缩可能 end session；默认 append，`replace` 为例外。
- Pointer：同会话同表；压缩 = 软标记 + append summary；**无客户端 replace**。
- Tool 行：Pointer 磁盘保留独立 `role: tool`；UI 用 inline 展示。

## 10. 测试要点

- meta-only save 不覆盖 messages
- append 后 FTS 可检索新内容
- `sync_messages_ordered_with_meta`：context_state 变更 + 插入 summary，总行数不减
- **压缩落库**：`persist_context_compression` — 前缀只更新 soft-exclude payload（position 不变）、后缀 `position += 1`、在切点插入 `ctx_*`；**禁止**对 drain 后的短列表做 `0..n-1` remap。内存可随后 drain。
- 若 `sync_ordered` 时 DB 仍有 history 之外的行（soft-exclude），改为保留已有 position、新行插入邻居前或 `max+1`，避免再次撞号。
- 问题回顾与契约说明见 [../developer/conversation-message-position-collision.md](../developer/conversation-message-position-collision.md)。
- `conversation_transcript`：tool 插入 anchor 后、reconcile 去掉 user 后 orphan
- `make_openai_messages_tests`：orphan tool 不上 wire
- **回归**：长对话多 tool 轮后 wire 顺序正确、无 HTTP 400

## 11. 后续扩展（未做）

- 按 message_id **删除** API（若需持久化「清错误气泡」）
- 多客户端同会话：仍依赖 SQLite `BEGIN IMMEDIATE`；内存 Registry 假设单活跃 run
- 将压缩/trim 以外所有 `conversation_store` 直写收口到 Registry

## 12. 相关文档

- [persistent-memory-and-self-improvement.md](persistent-memory-and-self-improvement.md) — MEMORY/USER 与 review
- [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md) — `conversations.db` 路径
