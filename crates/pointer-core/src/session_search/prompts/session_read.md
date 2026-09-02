### `session_read`

Read a window of stored messages (like file_read). No `query`.

Need `agentInstanceId` and/or `conversation_id`.
`conversation_id` alone is only for a past chat.

`offset` (1-based, default 1) or `around_message_id` — not both.
`limit` is message count (default 40, max 80).
Omitting `limit` does not read to the end.

No `window`, `role_filter`, or `tool_name`.

Returns `message_count`, `offset`, `returned`, `truncated`, `messages[]`.
