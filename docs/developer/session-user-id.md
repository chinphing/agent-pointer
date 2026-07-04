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

into terminal children when the stored id is non-empty.

Skills and shell scripts can read this variable via the **terminal** tool; it is not added to the LLM prompt by default.
