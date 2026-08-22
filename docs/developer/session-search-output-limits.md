# `session_search` 出站上限

工具回给模型的 JSON **不是**库里的原文。SQLite 仍保存完整消息。
桌面与 Web 共用 `pointer-core` 同一条路径。

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
`id` 仍在，可用 scroll 看同一条的更多上下文（仍受同一上限约束）。

侧栏会话搜索的 snippet 半径不变，与此工具出站上限不是同一套数字。

同一会话多条命中：`results` 仍是一项。工具回包 `matches[]` 最多 5 条
（`id` / `role` / `snippet`），`match_count` 为扫描窗口内去重条数。
只有主命中带 ±5 `messages`。

侧栏会话搜索不走这 5 条上限：点「N 处」列出该会话全部正文命中。

## 剔除旧 `session_search` 回包

按**工具名**识别，不在启动时全表扫 `content`。

写入 `role: tool` 时从对应 assistant `tool_calls[].name` 带上名字。
payload `toolName` 保留原串；`messages.tool_name` 列只存短名
（`mcp.session_search` → `session_search`），和 SQL `!= 'session_search'` 一致。
`session_search` 的索引列写成桩 `[session_search]`，完整回包仍在 `payload`。

Discovery / 窗口 / read：`tool_name != 'session_search'`，并排除已打桩的
`content`。没有工具名的旧行只对**已经取出来的那几条**看信封头，不扫全库。
旧库里未打名的巨型 FTS 行会等到该会话再次写入时才改成桩，启动时不回填。

## 观测

跳过旧回包、以及原文超过 `4 ×` 角色上限的截断打 **info**：
`skip_prior_tool_hit` / `omit_prior_tool_result` / `clip_hit_content`。

实现：`crates/pointer-core/src/conversation_store/search.rs`。
