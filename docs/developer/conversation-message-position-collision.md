# 会话消息 `position` 碰撞（上下文压缩）

> **状态**：已修复（2026-07，`persist_context_compression` + orphan-aware `sync`）。  
> **相关**：[`../design/conversation-store-append-migration.md`](../design/conversation-store-append-migration.md)

## 1. 现象

长会话多次上下文压缩后，SQLite `messages` 表出现大量 **同一 `position` 挂多条不同 `message_id`** 的行：

- `message_id` 本身不重复（`UNIQUE(conversation_id, message_id)` 仍成立）
- 碰撞对里「新」一侧常见为 `ctx_*`（压缩摘要 user 行），「旧」一侧多为更早的 `tool` / `assistant`
- 时间戳上 `ctx_*` 总是晚于被挤占的旧行
- `ORDER BY position ASC` 重载顺序不确定，搜索按 `position` 取窗也可能错乱

典型比例（一次真实库排查）：约 791 行消息里，数百个 `position` 发生碰撞。

## 2. 根因（不是「旧 sync 随便残留」）

`sync_messages_ordered_with_meta` 对传入列表做：

```text
INSERT … ON CONFLICT(conversation_id, message_id) DO UPDATE SET position = …
```

即 **按 `message_id` upsert**，并把列表下标写成 `0..n-1`。  
**不会 DELETE** 不在本次列表里的行。

压缩路径原先顺序是：

1. 前缀打 soft-exclude（`context_state.included = false`）
2. 在切点插入 `ctx_*` 摘要
3. **`history.drain(..split)`** 丢掉前缀（省内存）
4. **`sync_ordered(缩短后的 history)`** ← 问题点

短列表只有 `[ctx_, …后缀]`，被重写成低号 `position`；前缀旧行仍留在 DB 原号上 → **同一 `position` 两行**。  
每次压缩重复一次，碰撞越积越多。

```text
压缩前 DB:     pos0=A  pos1=tool  pos2=B
drain 后 sync: [ctx, B] → 写 pos0=ctx, pos1=B
结果:          pos0 = A 与 ctx 并存；pos1 = tool 与 B 并存
```

说明：

- soft-exclude 的设计本意是 **行仍留在 DB**（UI 可看、总行数不减），只是不喂模型
- 错的是 **用已 drain 的子集去做整表下标 remap**，不是「该不该留 exclude 行」

## 3. 排查时易走偏的方案

| 方案 | 结论 |
|------|------|
| sync 后 `DELETE … message_id NOT IN (短列表)` | 会物理删掉 soft-exclude 历史；lead 短列表还会误伤同会话 scoped 子 Agent 行 |
| 已存在行永不改 `position`，新行一律 `max+1` | 可消撞号，但 `ctx_` 会落到队尾，切点顺序错 |
| `ctx` 与切点消息共用 `position`，时间戳第二排序 | 与「B 前一条」共用 + `ASC` 理论上可排，但要改全链路 `ORDER BY`，且存量撞号仍乱 |
| 整会话 `DELETE` 再按内存 INSERT | 与 append-only / soft-exclude / 子 Agent 同行表冲突 |

## 4. 正确修法（当前实现）

### 4.1 压缩专用落库：`persist_context_compression`

在 **drain 之前**（内存仍含前缀 + 已插入的 `ctx_`）：

1. **UPDATE** 前缀已 mark exclude 的 payload（**不改**它们的 `position`）
2. 对切点及之后所有行：`position += 1`（整体后挪）
3. **INSERT** `ctx_*`，占用切点原号（严格小于后缀第一条 B）

然后再 `history.drain(..split)` 释放内存。  
**禁止**再对 drain 后短列表做 `0..n-1` 的 `sync_ordered`。

切点 = `insert_before_message_id`（`history[split]`，即保留段第一条）。

新消息（含并行子 Agent 追加）仍走既有规则：`position = MAX(position) + 1`（单事务内计算）。

### 4.2 后续 `sync_ordered` 防护

若 DB 中仍有 **history 之外** 的行（典型：已 drain 的 soft-exclude）：

- **不要**把短列表 remap 成 `0..n-1`
- 已存在 id：只更新 payload，**保留**原 `position`
- 新 id：插到后续邻居的 position 前（先后挪），否则 `max+1`

这样 tool-pass flush 不会再次用短列表撞上 exclude 行。

### 4.3 代码入口

| 模块 | 职责 |
|------|------|
| `context_compression.rs` | drain 前调用 `persist_compression_splice` |
| `conversation_transcript/mod.rs` | `persist_compression_splice` 包装 |
| `conversation_store/write.rs` | `persist_context_compression_in_conn`、orphan-aware `sync_messages_ordered_with_meta_in_conn` |

## 5. 与并行子 Agent

- 子 Agent 与压缩落库都经 `execute_write` / `BEGIN IMMEDIATE`，**同库写串行**，不会两个事务读到同一 `max` 各插一行。
- 主会话压缩与 `run_subagent` 并行 wave 通常时间错开（wave 期间 lead 在等结果）。
- 子 Agent 自身压缩（`CompressionScope::SubAgent`）**不走** `persist_context_compression`；scoped 行与 lead 同表，主压缩后挪时若 `position >= 切点` 会一并 +1，相对顺序保持。

## 6. 回归注意

- 压缩后：`COUNT(*)` 应为「原行数 + 1（summary）」；`GROUP BY position HAVING COUNT(*) > 1` 应为空；`ctx_*` 应夹在 exclude 前缀与保留后缀之间。
- 单测见 `conversation_store::write::tests`：`persist_compression_shifts_suffix_and_keeps_unique_positions`、`short_list_sync_with_db_orphans_does_not_collide_positions`。
- **存量库**已撞号的行不会自动消失；需要时另做数据修复（按 `created_at` / 是否 `ctx_` 等规则整理），本修复只阻止新增碰撞。

## 7. 契约（给后续改动）

1. soft-exclude 行必须能写回 DB（至少更新 `context_state`），且应保留独立 `position`。  
2. 在切点插入摘要时，保证 `ctx.position < 后缀第一条.position`（当前用后缀整体 +1）。  
3. 内存 drain 只为省 RAM，**不能**成为「DB 真相列表变短」的理由。  
4. 不要对「明显短于 DB 行集」的列表做稠密 remap；若必须全量重排，传入列表须含全部应保留 id（含 exclude 与 scoped，或改用显式定点 DELETE）。
