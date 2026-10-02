# `session_search` / `session_read` 出站上限

[English](../../en/developer/session-search-output-limits.md) | 简体中文

拆成两个工具的方案见 [`../design/session-search-scope-extension.md`](../design/session-search-scope-extension.md)（P0 已落地）。

工具回给模型的 JSON **不是**库里的原文。SQLite 仍保存完整消息。
桌面与 Web 共用 `pointer-core` 同一条路径。

侧栏会话搜索（UI）不走这两个工具，行为不变。

## 命中附近截断（按角色）

`content` 超限时按**查询词命中位置**截取，不是从文首切。
实现：`text_util::match_centered_excerpt`（约 1/4 在命中前，其余在后）。
无查询词（scroll / read）或正文中没有命中时，退回文首截断。

| 角色 | 上限（Unicode 字符） |
|------|----------------------|
| user | 4000 |
| assistant | 2500 |
| tool | 1500 |
| 其他 | 1500 |

超限时该消息带 `truncated`、`contentChars`（原文长度）、`contentLimit`。
`id` 仍在，可用 `session_read`（`around_message_id`）看同一条的更多上下文（仍受同一上限约束）。

侧栏会话搜索的 snippet 半径不变，与此工具出站上限不是同一套数字。

同一会话多条命中：`results` 仍是一项。工具回包 `matches[]` 最多 5 条
（`id` / `role` / `snippet`），顺序为助手 → 用户 → 其他 → 工具；
`match_count` 为扫描窗口内去重条数。主命中优先助手（不是工具输出）。
只有主命中带 ±5 `messages`。

侧栏会话搜索不走这 5 条上限：点「N 处」列出该会话用户/助手正文命中（不含 tool）。

## 剔除旧 `session_search` / `session_read` 回包

按**工具名**识别，不在启动时全表扫 `content`。

写入 `role: tool` 时从对应 assistant `tool_calls[].name` 带上名字。
payload `toolName` 保留原串；`messages.tool_name` 列只存短名
（`mcp.session_search` → `session_search`），和 SQL `NOT IN ('session_search','session_read')` 一致。
索引列写成桩 `[session_search]` / `[session_read]`，完整回包仍在 `payload`。

Discovery / 窗口 / read：跳过这两类工具名，并排除已打桩的
`content`。没有工具名的旧行只对**已经取出来的那几条**看信封头，不扫全库。
旧库里未打名的巨型 FTS 行会等到该会话再次写入时才改成桩，启动时不回填。

搜索时 **不要** `SELECT messages.content` 做 FTS 排序，也 **不要** 用 `content LIKE '%词%'` 扫会话。
生产库里未打桩的旧回包可达数十万字符；每敲一次侧栏搜索把这些 blob 拉进内存就会明显变慢。

约定：

- 侧栏 MATCH：`{content}: (查询) NOT {role}: tool`（`role` 列可检索；查询词只打在 `content` 上，避免搜 `assistant` 命中所有助手行）。`session_search` 工具仍用不带该条件的 MATCH。
- 侧栏 FTS 按会话聚合，不要 `bm25` 给整库打分，也不要用全局 `rowid LIMIT` 丢掉旧会话。JOIN `lower(m.role) != 'tool'` 作兜底。
- 侧栏首次搜索每个会话只取 **一条** 主命中 snippet（助手优先）；`match_count` 是该会话非 tool 的 FTS 条数。点「N 处」再拉完整列表。
- 升级后 `store_meta.fts_schema=role_indexed` 会重建 FTS（大库首次打开可能较久）。若重建失败或索引为空，下次打开会重试，**不要**在重建成功前写入 schema 标记。新消息仍进同一张 `messages_fts`（含 tool）。`session_search` 回包索引列为桩 `[session_search]`。
- 完整列表用 **一次 FTS MATCH**，只把正文**前缀**读进内存做 snippet，不用 LIKE 全表扫描，也不按会话循环拉整段 blob。
- `session_search` discovery 先分组出会话，再按命中 id 回表取正文做 snippet / ±`window` 窗口。带 `agentInstanceId` 时窗口与 `session_read` 一样只含该线程（`messages.agent_instance_id` 列）。无 bookend。
- `session_read` 按 `offset` 或 `around_message_id` 取消息窗口（默认 40、顶 80）。

观测：`ui search: … rank_ms= matches_ms= elapsed_ms=`、`session_search: discover … elapsed_ms=`、`session_read: …`。

对本机库计时（只读，不改 schema）：

`POINTER_PROFILE_DB="$HOME/Library/Application Support/PointerAppDev/conversations.db" cargo test -p pointer-core profile_ui_search_real_db -- --ignored --nocapture`

## 观测

跳过旧回包、以及原文超过 `4 ×` 角色上限的截断打 **info**：
`skip_prior_tool_hit` / `omit_prior_tool_result` / `clip_hit_content`。

实现：`crates/pointer-core/src/conversation_store/search.rs`。
