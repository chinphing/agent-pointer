# Session user id

Each conversation row stores `session_user_id` (API field `sessionUserId`).

## Resolution

| Source | Value |
| --- | --- |
| Desktop / web chat | Platform OAuth user id (`platform_user_id`) on first `run_chat` when empty |
| IM direct message | Channel `sender_id` (e.g. WeCom `userid`, Feishu `open_id`) |
| IM shared group session | Fixed `conversation_key` (e.g. `wecom:group:{chatId}`) |

IM values are written on each inbound message before `run_chat`. Desktop values use `ensure` and do not overwrite an existing id.

## `session_search` isolation

The host injects `_session_user_id` (from the active conversation row) into every
`session_search` call. Browse, discovery, scroll, and read only return conversations
whose stored `session_user_id` matches (trimmed equality). Empty matches empty
(anonymous / legacy rows). Cross-user targets return the same `conversation_id not found`
error as a missing id.

Browse uses index `idx_conversations_user_updated (session_user_id, updated_at_ms DESC)`.
Values are normalized on write (`trim`); discovery joins `conversations` on `id` with
`session_user_id = ?` (no `trim()` on columns).

## Subprocess env

When the stored id is non-empty, Pointer injects `SESSION_USER_ID` into **`terminal`** child processes.
See [terminal-environment-variables.md](terminal-environment-variables.md) for injection rules, thread-local guards, and script usage.

**Requires a non-empty stored id:** desktop/Web need **平台账户** OAuth login so the first `run_chat` can persist `platform_user_id`; IM sessions use channel sender / group key. API-key-only chats without platform login leave `session_user_id` empty and the variable unset.
