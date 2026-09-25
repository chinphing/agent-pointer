### `session_search`

Find text in stored transcripts (like file_grep). `query` is required.

Optional:
- `conversation_id` — one past chat
- `agentInstanceId` — one lead or child thread.
  Omit it to search every chat, including this chat's lead.
  This chat's child threads stay out until you pass their id.
- `role_filter` — e.g. `user,assistant,tool`
- `tool_name` — e.g. `terminal` (comma-separated)
- `limit` — conversation groups (default 3, max 10)
- `window` — ± messages around the primary hit
  in the same locator slice (default 5, max 20).
  With `agentInstanceId`, the window stays in that thread.

No `offset` or `around_message_id` — use session_read.

Results group by conversation.
Hits include `match_message_id` and `agentInstanceId`.
Filter by tool type here, then session_read around the hit id.
