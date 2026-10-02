# `session_search` / `session_read` outbound limits

English | [简体中文](../../zh-CN/developer/session-search-output-limits.md)

The plan for splitting this into two tools is in [`session-search-scope-extension.md`](../../zh-CN/design/session-search-scope-extension.md) (P0 shipped).

The JSON the tool returns to the model is **not** the original text in the database. SQLite still stores the full messages.
Desktop and Web share the same `pointer-core` path.

Sidebar conversation search (UI) does not go through these two tools; its behaviour is unchanged.

## Hit-centred truncation (by role)

When `content` exceeds the limit it is cut around the **query-term hit position**, not from the start of the text.
Implementation: `text_util::match_centered_excerpt` (about 1/4 before the hit, the rest after).
With no query term (scroll / read), or when the body contains no hit, it falls back to truncating from the start.

| Role | Limit (Unicode chars) |
| --- | --- |
| user | 4000 |
| assistant | 2500 |
| tool | 1500 |
| other | 1500 |

When over the limit the message carries `truncated`, `contentChars` (original length) and `contentLimit`.
`id` is still present; use `session_read` (`around_message_id`) to see more context for the same message (still subject to the same limit).

The sidebar conversation search snippet radius is unchanged; it is not the same set of numbers as this tool's outbound limits.

Multiple hits in one conversation: `results` is still a single entry. The tool response's `matches[]` holds at most 5 entries
(`id` / `role` / `snippet`), ordered assistant → user → other → tool;
`match_count` is the de-duplicated count within the scan window. The primary hit prefers assistant (not tool output).
Only the primary hit carries ±5 `messages`.

Sidebar conversation search does not use this 5-entry cap: clicking "N matches" lists that conversation's user/assistant body hits (excluding tool).

## Dropping old `session_search` / `session_read` responses

Identified by **tool name**; no full-table scan of `content` at startup.

When writing `role: tool`, carry the name over from the corresponding assistant `tool_calls[].name`.
The payload's `toolName` keeps the original string; the `messages.tool_name` column stores only the short name
(`mcp.session_search` → `session_search`), consistent with the SQL `NOT IN ('session_search','session_read')`.
The index column is written as the stub `[session_search]` / `[session_read]`; the full response stays in `payload`.

Discovery / window / read: skip these two tool names, and exclude the already-stubbed
`content`. Old rows with no tool name are checked for the envelope header only for **the rows already fetched**, with no full-database scan.
Giant unnamed FTS rows in an old database are converted to stubs only when that conversation is written to again; there is no backfill at startup.

When searching, do **not** `SELECT messages.content` for FTS ranking, and do **not** scan conversations with `content LIKE '%term%'`.
Unstubbed old responses in a production database can reach hundreds of thousands of characters; pulling those blobs into memory on every sidebar search keystroke would visibly slow things down.

Conventions:

- Sidebar MATCH: `{content}: (query) NOT {role}: tool` (the `role` column is searchable; the query term is applied to `content` only, to avoid searching `assistant` and hitting every assistant row). The `session_search` tool still uses a MATCH without that condition.
- Sidebar FTS aggregates by conversation: do not score the whole database with `bm25`, and do not use a global `rowid LIMIT` that drops old conversations. JOIN `lower(m.role) != 'tool'` as a fallback.
- The sidebar's first search takes only **one** primary hit snippet per conversation (assistant preferred); `match_count` is that conversation's non-tool FTS count. Clicking "N matches" then fetches the full list.
- After an upgrade, `store_meta.fts_schema=role_indexed` rebuilds the FTS (the first open of a large database may take a while). If the rebuild fails or the index is empty, the next open retries; do **not** write the schema marker before the rebuild succeeds. New messages still go into the same `messages_fts` (including tool). The `session_search` response's index column is the stub `[session_search]`.
- The full list uses **one FTS MATCH**, reading only the body **prefix** into memory for the snippet — no LIKE full-table scan, and no per-conversation loop pulling whole blobs.
- `session_search` discovery first groups out conversations, then goes back to the table by hit id to fetch the body for the snippet / ±`window` window. With `agentInstanceId` the window contains only that thread, like `session_read` (the `messages.agent_instance_id` column). No bookend.
- `session_read` fetches a message window by `offset` or `around_message_id` (default 40, max 80).

Observability: `ui search: … rank_ms= matches_ms= elapsed_ms=`, `session_search: discover … elapsed_ms=`, `session_read: …`.

Timing against a local database (read-only, no schema change):

`POINTER_PROFILE_DB="$HOME/Library/Application Support/PointerAppDev/conversations.db" cargo test -p pointer-core profile_ui_search_real_db -- --ignored --nocapture`

## Observability

Skipping old responses, and truncating bodies over `4 ×` the role limit, log **info**:
`skip_prior_tool_hit` / `omit_prior_tool_result` / `clip_hit_content`.

Implementation: `crates/pointer-core/src/conversation_store/search.rs`.
