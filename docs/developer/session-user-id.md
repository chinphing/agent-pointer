# Session user id

Each conversation row stores `session_user_id` (API field `sessionUserId`).

## Resolution

| Source | Value |
| --- | --- |
| Desktop / web chat | Platform OAuth user id on first `save_conversation_meta` (new session), `save_chat_attachment`, or `run_chat` when empty |
| Standalone password login | Fixed `local-admin` (single ops identity — not for multi-user isolation) |
| Standalone SSO (`?sso=`) | Ticket claim `sub` (use distinct `sub` per person for different `SESSION_USER_ID`) |
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
| `projects.session_user_id` | Project ownership; mutate scoped to owner; list visibility uses `ListScope` below |

## Sidebar / project list visibility (`ListScope`)

Conversation meta list (**最近** sidebar), sidebar search, project list/sidebar/get, and project
conversation metas use `ListScope` derived from the active platform session:

| Viewer | Scope | Visibility |
| --- | --- | --- |
| Platform admin (`is_platform_admin`, e.g. standalone password `local-admin`) | `All` | Every user's conversations and projects |
| Non-admin (SSO `sub`, OAuth user, etc.) | `User(uid)` | Only rows with matching `session_user_id` |

Create / update / delete of projects remain owner-scoped (admin “see all” does not
grant mutate-others). Opening a conversation by id still loads messages by id;
harden IDOR separately if needed.

### Projects (multi-user)

`projects` rows store `session_user_id` (API `sessionUserId`). Empty means legacy/anonymous.

- List / sidebar / get / project conversation list follow `ListScope` (admin sees all).
- Create / update / delete remain scoped to the current platform user
  (`SSO sub` / OAuth id / `local-admin`).
- Each non-empty user gets their own **默认项目** whose `workspaceRoot` is
  `{session-sandboxes}/{session_user_id}/`.
- Same folder path may create distinct projects for different users (no cross-user reuse).
- Legacy unowned projects (`session_user_id=''`) remain visible only when the current uid is empty;
  if such a row already points at a user's sandbox, the first reconcile for that user may claim it.

Legacy root-level `memories/MEMORY.md` is read as a fallback until a user-scoped file exists. New writes always go to the user subdirectory.

`save_conversation_meta` binds `session_user_id` when the platform session is logged in and the row is still empty (covers new sessions before the first message). `save_chat_attachment` requires login and binds before writing files, so attachments are not stored under `_anonymous/` after login.

During agent runs, new-layout media paths are checked against the active `SESSION_USER_ID` when loading files. Legacy `{conversation_id}/…` and `_anonymous/…` paths remain readable.
