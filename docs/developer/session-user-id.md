# Session user id

Each conversation row stores `session_user_id` (API field `sessionUserId`).

## Resolution

| Source | Value |
| --- | --- |
| Desktop / web chat | Platform OAuth user id (`platform_user_id`) on first `run_chat` when empty |
| IM direct message | Channel `sender_id` (e.g. WeCom `userid`, Feishu `open_id`) |
| IM shared group session | Fixed `conversation_key` (e.g. `wecom:group:{chatId}`) |

IM values are written on each inbound message before `run_chat`. Desktop values use `ensure` and do not overwrite an existing id.

## Subprocess env

During an agent run, Pointer sets thread-local context and injects:

```bash
SESSION_USER_ID=<session_user_id>
```

into **terminal** child processes when the stored id is non-empty. Because terminal runs on a blocking thread pool, the runtime re-reads `session_user_id` from the conversation row before each tool call (same pattern as workspace root).

**Requires a non-empty stored id:** desktop/Web need **平台账户** OAuth login so the first `run_chat` can persist `platform_user_id`; IM sessions use channel sender / group key. API-key-only chats without platform login leave `session_user_id` empty and the variable unset.
