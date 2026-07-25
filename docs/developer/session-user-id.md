# Session user id

Each conversation row stores `session_user_id` (API field `sessionUserId`).

## Resolution

| Source | Value |
| --- | --- |
| Desktop / web chat | Platform OAuth user id on first `save_conversation_meta` (new session), `save_chat_attachment`, or `run_chat` when empty |
| Standalone password login | Fixed `local-admin` |
| Standalone SSO (`?sso=`) | Ticket claim `sub` (third-party user id) |
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

**Requires a non-empty stored id:** desktop/Web platform OAuth, standalone SSO `sub`, or IM sender/group key. Standalone password-only sessions use `local-admin`. API-key-only chats without login leave `session_user_id` empty and the variable unset.

## On-disk layout (user-scoped)

| Path | Layout |
| --- | --- |
| `memories/{session_user_id}/MEMORY.md` | Agent notes for one user; `_anonymous/` when id empty |
| `memories/{session_user_id}/USER.md` | User profile for one user |
| `session-sandboxes/{session_user_id}/attachments/{12-hex-id}_{file}` | New chat attachments and AI-generated media |
| `session-sandboxes/_anonymous/{conversation_id}/attachments/{12-hex-id}_{file}` | New anonymous attachments and AI-generated media |
| `conversation-media/…`, `generated-media/…` | Historical media; read-compatible and not migrated automatically |

Legacy root-level `memories/MEMORY.md` is read as a fallback until a user-scoped file exists. New writes always go to the user subdirectory.

`save_conversation_meta` binds `session_user_id` when the platform session is logged in and the row is still empty (covers new sessions before the first message). `save_chat_attachment` requires login and binds before writing files, so attachments are not stored under `_anonymous/` after login.

During agent runs, new-layout media paths are checked against the active `SESSION_USER_ID` when loading files. Legacy `{conversation_id}/…` and `_anonymous/…` paths remain readable.
