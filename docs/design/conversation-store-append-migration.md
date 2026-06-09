# 对话存储：逐条 Append 迁移（P0–P2）

> **状态**：**P0–P2 已实现**（见下方模块与 API）。P0 后端 chat stream 落库；P1 前端 meta-only；P2a 压缩/trim sync；P2b 前端 replace。

## 1. 背景

| | 迁移前 | 目标（Hermes 式） |
|--|--------|-------------------|
| 主存储 | `conversations.json` → `conversations.db` | 同上，SQLite 为唯一真相源 |
| 写入路径 | 前端 `persist()` debounce 400ms，**全量** `save_conversations` | 后端 append 为主；前端只写 meta |
| 单次聊天 I/O | DELETE 会话全部 messages + INSERT 全量 | 通常 1 条 INSERT / UPDATE |
| FTS | 与 messages 同事务 | 触发器随 INSERT/UPDATE 同步 |

## 2. 三阶段定义（Pointer 修正版）

```text
P0  chat stream ──append/upsert──► messages 表
P1  前端 UI     ──patch meta──► conversations 表字段（不带 messages）
P2  例外路径：
      P2a  patch + append — 压缩 / TaskBoard trim（改 context_state，插入 summary）
      P2b  replace         — undo / retry / 物理删改 transcript
```

### P0：后端在聊天流里逐条落库

在 `pointer-core` 聊天循环中，对**主会话** `history` 的变更写库：

- `run_chat` 开始：`append_missing_messages`（补写前端已发、库中尚无的消息）
- 助手回合结束（`commit_lead_assistant_turn`）：`upsert_message`
- 工具结果入 `history`：`upsert_message`
- 压缩 / trim 成功：**P2a**（见下）

过渡期可与前端双写；稳定后前端不再 bulk 写 messages。

### P1：前端 `persist()` 只做元数据同步

前端只同步 **会话壳** 字段：

- `title`、`updatedAt`
- `workspaceRoot`、`computerMonitorId`
- `toolRoundsUsed` / `toolRoundsUsedSupervisor`
- `skillIds`（字段保留，UI 基本未用）

API：`save_conversation_meta(metas[])`；删除会话仍通过 meta 列表 diff（`delete_conversations_not_in`）。

**功能入口**（`src/stores/chat.ts`）：

| 入口 | 说明 |
|------|------|
| `newConversation` / `deleteConversation` | 会话生命周期 |
| `setConversationWorkspace` / `applyPersistedComposerDefaults` | Composer 工作目录 |
| stream `workspace_updated` / `computer_monitor_updated` | 后端确认 meta |
| stream `done` | 工具轮次计数（meta） |
| stream `message_end` | 自动标题（meta 部分） |

### P2：Pointer 版例外路径

**不是** Hermes 式「压缩 = replace_messages」。

Pointer 压缩（`context_compression.rs`）：

1. 旧消息 **保留**，仅 `context_state.included = false`
2. **插入** summary user 消息
3. LLM 侧用 `filter_context_messages()` 过滤；磁盘 transcript 完整

| 子类 | 存储行为 | 场景 |
|------|----------|------|
| **P2a** | `sync_messages_ordered`：按序 upsert payload/position，**不 DELETE** | 压缩、`TaskBoardTrim` |
| **P2b** | `replace_messages`：DELETE 全消息 + INSERT | `undo`、`retry`、清除错误气泡 |

## 3. 模块与 API

```
crates/pointer-core/src/conversation_store/
├── persist.rs    # load + 低层 SQL
├── write.rs      # append / sync / replace / meta
└── mod.rs        # ConversationStore 公开方法

crates/pointer-core/src/chat_service/
└── conversation_persist.rs   # run_chat 挂点，warn 日志
```

| Rust API | 用途 |
|----------|------|
| `append_missing_messages` | P0 |
| `upsert_message` | P0 |
| `sync_messages_ordered` | P2a |
| `replace_messages` | P2b |
| `save_meta_all` | P1 |
| `replace_conversation_messages`（HTTP/Tauri） | 前端 undo 等 |

跨入口：Tauri command + Web `PUT /api/conversations/meta`、`PUT /api/conversations/:id/messages`。

## 4. 与 Hermes 差异

- Hermes：压缩可能 end session + child session；默认 `append_message`，`replace_messages` 为例外。
- Pointer：**同一会话、同表**；压缩 = **软标记 + append summary**；`replace_messages` 仅 P2b。

## 5. 测试要点

- meta-only save 不覆盖 messages
- append 后 FTS 可检索新内容
- sync_messages_ordered：context_state 变更 + 中间插入 summary，message 总数不减
- replace_messages：undo 后 DB 与内存一致

## 6. 相关文档

- [persistent-memory-and-self-improvement.md](persistent-memory-and-self-improvement.md) — MEMORY/USER 与 review
- [../guides/cross-platform-build.md](../guides/cross-platform-build.md) — `conversations.db` 路径
