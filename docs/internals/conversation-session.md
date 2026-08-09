# Conversation session facade

In-process owner of lead transcript mutations and the LLM working-set cache.

## Why

Historically many paths wrote SQLite messages directly (`append_missing`,
`upsert`, compression, channels, FE persist). `run_chat` then reloaded the
full working set every turn for correctness. That made “skip reload” unsafe.

## Rules

1. **Mutate transcript via `conversation_session`** (`append_missing`,
   `upsert_message`, `sync_ordered`, `persist_compression_splice`, …).
2. `ConversationStore` message writes still exist for low-level/tests, but each
   write calls `note_transcript_mutated` so the facade cache cannot go stale.
3. **UI hydrate / FTS / meta** may read SQLite directly; they are not the lead
   working set.
4. `prepare_lead_history` is the only path `run_chat` should use to build lead
   `history` at turn start.

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
