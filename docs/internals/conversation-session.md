# Conversation session facade

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
   working set. Legacy full import may still use public `save_all`.
4. `prepare_lead_history` is the only path `run_chat` should use to build lead
   `history` at turn start.

## Observability: `save_all` (legacy)

`ConversationStore::save_all` logs at **info** with a stable prefix:

`conversation_store: save_all_legacy_import …`

Search production logs for that string. If it never appears for a release window,
consider removing the FE/server `save_conversations` API and tightening
`save_all` further. Do not remove based on silence in debug-only logs.

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
