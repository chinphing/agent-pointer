# 会话检索拆成 `session_search` + `session_read`

> **状态**：P0–P1 已实现（`session_search` + `session_read`、`lead_agent_instance_id`、子回包 `agentInstanceId`、`messages.agent_instance_id` 列 + 索引）。  
> **形态**：两个工具，对齐 **`file_grep` / `file_read`**。定位器、角色、工具名是参数，不是第三种 mode。  
> **读者**：核心维护者。落地后同步英文短行 prompt（不写仓库路径）与 [`../developer/session-search-output-limits.md`](../developer/session-search-output-limits.md)。

侧栏会话搜索（UI）**不改**。

---

## 1. 为什么拆两个

单工具靠「有没有 `query`」分派，模型和 schema 仍会把 `limit` / `offset` / `window` 混用（discovery = 会话数，read = 消息数）。文件栈已经用拆工具躲开这件事。

拆开之后：

| 工具 | 类比 | 必填 | `limit` |
|------|------|------|---------|
| **`session_search`** | `file_grep` | `query` | 最多几个会话分组（现网默认 3、顶 10） |
| **`session_read`** | `file_read` | 至少一个定位器 | 从 `offset` 起最多多少条消息（建议默认 40、顶 80） |

`session_search` **保留现名**（模型已认识）；不要改成 `session_grep`。新工具只加 **`session_read`**。

现网 browse / 四形态 scroll **删除**。scroll 变成 `session_read` 的 `around_message_id` 或 `offset`。无参列表走侧栏。

不新开 `job.trace`。`job` 只管生命周期（`jobId`）；**一条 agent 线程**的身份用 **`agentInstanceId`**（父、子同一套，对齐 Codex `thread_id` / Hermes `session_id`）；正文用这两个工具。`traceId` 仍给 UI / 内部拼装，不作为模型主句柄。**子不新建 `conversation_id`。** `run_id` 仍是一轮 `run_chat`（对齐 Codex `turn_id`），不当切片键。

## 1.1 统一字段：`agentInstanceId`

**已拍板：lead instance 绑在「这条 lead 线程」上，不绑在每次 `run_chat`。**

| 角色 | 何时 mint | 何时复用 | 何时换 |
|------|-----------|----------|--------|
| **Lead** | 会话还没有 lead instance 时 | 之后每一次 `run_chat` / 溢出重跑 `run_chat_inner` | 用户 **切换 `lead_agent_id`**（如 general→coder）时新 mint；`/new` 新会话本身就是新 conversation |
| **子** | `run_subagent` spawn（后台在 `register` 时 mint 并传入 child） | 该子循环结束前 | 下一次 spawn 一定新 UUID（含同一 `taskId` 续跑） |

落库：`conversations` 增加 **`lead_agent_instance_id`**（可空，旧会话第一次 `run_chat` 再填）。`ChatLlmTokenSession` 用 `AgentInstanceScope::with_instance_id`，禁止每轮 `new()`。lead 写出的消息 stamp 这个 id。侧栏换 agent（`save_conversation_meta`）与 IM `patch_session_agent` 在 `lead_agent_id` 真变时都旋转 instance。

压缩不旋转 instance（不学 Hermes 压缩切 `session_id`）。token / 压缩日志仍可用同一把。

**对外只用 `agentInstanceId`。** `session_read` / `session_search` 切该线程写出的行 = `payload.agentInstanceId`。不要用 `traceId` 当模型句柄。

| 候选 | 角色 |
|------|------|
| **`agentInstanceId`** | 父、子共同的线程 id；切片主键 |
| `conversation_id` | 用户那一条聊天（父子共用） |
| `run_id` | 一轮 turn；观测 / 工具预算 |
| `jobId` | 仅后台等待/取消 |
| `taskId` | 仍不回给父模型 |
| `traceId` | UI / 内部 stamp |

### 还要补的

子行已经 stamp `agentInstanceId`。父行默认没有。

1. 持久化 lead instance + 写出 lead 消息时 stamp。旧会话无列值：第一次跑 mint，**不回填**历史消息（旧行只能 `conversation_id` 读）。
2. 子回包带 `agentInstanceId`；后台 mint 一次传入 child。
3. 可选：system 一行 `Your agentInstanceId is …`。读孩子看委派回包即可。
4. `session_read(agentInstanceId)` 不看 `context_included`。当前会话无 instance、只带 conversation → search 不要扫 lead；read **拒绝**。
5. Prompt：instance 只用于 search/read，不要传给 `run_subagent`。

P0：`json_extract` + 当前 `conversation_id`。P1：`messages.agent_instance_id` 列 + 索引（已落地；检索走列，payload 仍 stamp 同一字段）。

## 2. 参数必须两套 schema

实现可共用切片 SQL。**工具 JSON / prompt / `tools[]` 字段分开**，不要做成「同一组参数靠有没有 query 分派」。

当前会话只传 `conversation_id`、不传 `agentInstanceId`：

- `session_search`：不要搜当前会话 lead。
- `session_read`：**拒绝**。读本会话 lead 用 lead 的 `agentInstanceId`。

剔除名为 `session_search` / `session_read` 的工具回包。

## 3. `session_search`（grep）

| 参数 | 必填 | 默认 | 含义 |
|------|------|------|------|
| `query` | 是 | — | FTS5。缺则错误 |
| `conversation_id` | 否 | 不限（跳过当前 lead） | 收窄到该历史会话。`session_id` 别名 |
| `agentInstanceId` | 否 | 不限线程 | 收窄到该 lead/子线程（默认当前会话） |
| `role_filter` | 否 | 不限 | `user,assistant,tool` |
| `tool_name` | 否 | 不限 | 如 `terminal`，逗号分隔 |
| `limit` | 否 | 3，顶 10 | 最多几个**会话分组** |
| `window` | 否 | 5，顶 20 | 主命中 ±N（带 `agentInstanceId` 时只含该线程） |

**没有** `offset`、`around_message_id`。回包按会话分组；命中带 `agentInstanceId`、`match_message_id`。不要 bookend。

只要某类工具：在 search 上过滤，再用命中 id 去 read。

## 4. `session_read`（窗口）

**禁止 `query`。**

| 参数 | 必填 | 默认 | 含义 |
|------|------|------|------|
| `agentInstanceId` | 与下行至少一 | — | 读该线程（当前会话默认） |
| `conversation_id` | 与上行至少一 | — | **仅历史会话**可单独当定位器。`session_id` 别名 |
| `offset` | 否 | 1 | 过滤后时间序、1-based。与 `around_message_id` **互斥** |
| `limit` | 否 | 40，顶 80 | 最多几条**消息**。省略不是读到末尾 |
| `around_message_id` | 否 | — | 从该消息起读 `limit` 条（须属于该定位器） |

**没有** `query`、`window`、`role_filter`、`tool_name`。回包：`message_count`、`offset`、`returned`、`truncated`、`messages[]`。出站截断同现网。

## 5. 场景

| 意图 | 工具 |
|------|------|
| 以前聊过 X | `session_search`(`query`) |
| 在旧会话里搜 | `session_search`(`query`, `conversation_id`) |
| 读该会话一段 | `session_read`(`conversation_id`, `offset`/`limit`) |
| 子线里搜错误串 | `session_search`(`query`, `agentInstanceId`) |
| 核对过程 | `session_search`(`query`, `agentInstanceId`, `role_filter=tool`) 再 `session_read`(`around_message_id`)；或直接 `session_read`(`agentInstanceId`) 看窗口 |
| 翻某次 launch 中间 | `session_read`(`agentInstanceId`, `around_message_id`) |
| 读本会话 lead 已 stamp 的行 | `session_read`(`agentInstanceId` = 本会话 lead) |
| 文件 / 记忆 / job 是否结束 | `file_*` / `memory` / `job` |

handoff 已够则不要 `session_read` 过程。并行多个 `self` 必须带**那一个** `agentInstanceId`。等待后台仍用 `job.await`（`jobId`）。

## 6. 评估（相对「一个工具两 mode」）

**更好**

- `limit` / `offset` / `window` / `query` 各出现在一个 schema 里，native `tools[]` 描述不会互相踩。
- 模型已有 `file_grep` → `file_read` 习惯：先 search 再 read。
- 不会「想读窗口却带了 query」被分派成 grep——那是调错工具，宿主直接报错。
- 实现分两个 `ToolEntry`、两份短 prompt；共享切片函数。Tools appendix 用同一 `doc_source`（recall）只贴一份家族文档。

**代价**

- prompt 两套短表：search 只有 query/过滤/`window`；read 只有定位器/`offset`/`limit`/`around_message_id`。
- Agent 工具表、并行白名单（`parallel.rs` 只读类）、FTS 打桩、测试、`plan_includes_session_search` 要扩成「search 或 read」。
- 旧会话里只有 `session_search` 的 scroll/read 用法会失效，需 prompt + 报错引导到 `session_read`。

**不采用**

- 改名为 `session_grep`：无收益，打断已有工具名。
- 第三个 `session_list`：回到 browse。

## 7. 实现要点

- 模块仍在 `session_search/`：`tool.rs` 注册两个 entry，或 `search_tool.rs` + `read_tool.rs`。
- 继承：与现网 `session_search` 相同（默认可进子 Agent）。self leaf 很少需要；不单独关 inheritance。
- 并行：`session_read` 与 `session_search` 一样标只读可并行。
- 切片键：`agentInstanceId`（见 §1.1）。`job.await` 与 `session_read` 不要混用。
- 错误不静默。观测：`session_search:` / `session_read:` 两条日志。
- App / Web / 三平台：无分支。

## 8. 非目标 / 分期

非目标同前：不改侧栏、不注入子历史、不跨用户。

| 阶段 | 内容 |
|------|------|
| **P0** | `conversations.lead_agent_instance_id`；`run_chat` 复用；切换 lead 换 id（`patch_session_agent` 与 `save_conversation_meta` 都旋转）；消息 stamp；子回包 + 后台 mint 一次；`session_read`；prompt + 测试 |
| **P1** | 物化 `messages.agent_instance_id` 列 + 索引（从 payload 回填已 stamp 的行，不发明历史 id） |
| **P2** | 仅 `agent_id` 的 ambiguous candidates |

对照：[`async-subagent-and-terminal.md`](async-subagent-and-terminal.md)、[`../developer/pointer-run-subagent.md`](../developer/pointer-run-subagent.md)、[`../developer/session-search-output-limits.md`](../developer/session-search-output-limits.md)。
