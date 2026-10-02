# 会话门面（Conversation session）

In-process owner of lead transcript mutations and the LLM working-set cache.

## Why

Historically many paths wrote SQLite messages directly (`append_missing`,
`upsert`, compression, channels, FE persist). `run_chat` then reloaded the
full working set every turn for correctness. That made “skip reload” unsafe.

## Rules

1. **Mutate transcript via `conversation_session`** (`append_missing`,
   `upsert_message`, `sync_ordered`, `persist_compression_splice`, …).
2. `ConversationStore` message writes (`append_missing_messages`,
   `upsert_message*`, `sync_messages_ordered_with_meta`,
   `persist_context_compression`, `replace_messages`) are **`pub(crate)`** —
   external crates cannot call them. Inside `pointer-core` they remain for the
   facade and unit tests; each write still calls `note_transcript_mutated`.
3. **UI hydrate / FTS / meta** may read SQLite directly; they are not the lead
   working set.
4. `prepare_lead_history` is the only path `run_chat` should use to build lead
   `history` at turn start.

## Removed: legacy `save_conversations` / `save_all`

The FE/server `save_conversations` API (Tauri command + `PUT /api/conversations` +
`tauri.ts`/`web.ts` wrappers) was **removed 2026-08**: after the append-migration,
production logs never showed the legacy import marker, so the full-conversation
import path had no caller. `ConversationStore::save_all` is now `#[cfg(test)]`
(test convenience write only). Do not reintroduce a full-transcript client
overwrite API — use `conversation_session` writes instead.

## Cache

- Key: `conversation_id`
- Value: lead working-set `Vec<ChatMessage>` + `db_count` + `generation`
- Invalidate: any store transcript mutation (generation bump)
- Republish: after successful `run_chat` end, sync, compression drain, or
  cache miss reload
- LRU cap: 16 conversations

## Related

- Working-set SQL filter: [`long-chat-memory.md`](long-chat-memory.md)
- Compression persist: [`context-compression.md`](context-compression.md)
