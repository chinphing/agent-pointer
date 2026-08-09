# Long-chat memory (peak working set)

How Pointer reduces **peak RSS during a chat turn** without changing the persisted transcript model.

## Peak vs baseline

| Phase | What dominates heap |
|-------|---------------------|
| Peak (during LLM round) | Authoritative `history` + ephemeral injects + OpenAI JSON Values + HTTP wire body (and formerly a full `history` clone held across the stream) |
| Baseline (after Done) | Pinia / store transcript; inject base64 is not part of durable history |

`[CUR_SCREEN]` / `images_base64` are **wire-only**. They are appended to `injected_tail`, never written to session `history` / DB. UI uses `computerRoundScreenRelPath` + on-disk captures.

## Phase 1 (implemented)

1. **`MessageLoopPromptsAfterContext`** — `base_messages: &[ChatMessage]` (read-only) + `injected_tail: &mut Vec<ChatMessage>`. Hooks must only push onto the tail.
2. **Wire before spawn** — `OpenAIProvider::build_stream_chat_wire(base, injected_tail, …)` then `stream_chat_wired`. The spawned HTTP task holds wire JSON only, not a full `ChatMessage` history clone for the stream lifetime.
3. **Shorter Value overlap** — After `chat_request_wire_json`, `openai_msgs` / `ChatRequest` drop before the HTTP round-trip (`chat_once` and stream wire build).
4. **Metrics** — `log::info!` on pre-stream prep and `stream_chat_wire_built` with `cloned_history=false`, inject counts, and image slot counts.

## Phase 2.1 (implemented)

- Frontend `persistAppend` uses the same `persistedMessageIdsByConv` watermark as
  hydrated `sendChat`: only non-persisted rows are deep-cloned for
  `append_conversation_messages` (`messagesForPersistAppend`). Empty incremental
  skips the IPC/HTTP append but still refreshes the watermark.

## Phase 2.2 (implemented) — `run_chat` working-set history

- `ConversationTranscriptSession::begin` loads the **lead LLM working set** only
  (`context_state.included`, non-scoped) via `load_lead_working_messages`.
- Soft-excluded rows remain in SQLite for UI paging; meta `message_count` stays
  the full DB count so FE hydrate is unchanged.
- Sync paths already preserve DB orphans when the in-memory list is shorter
  (`sync_messages_ordered` preserve mode).

## Phase 2.2b (implemented) — `context_included` column (schema v22)

- `messages.context_included` mirrors `is_context_included` (soft-exclude + scoped).
- Writes set the column on insert/upsert; one-time set-based JSON backfill is
  gated by `store_meta.context_included_backfilled`.
- Working-set load uses `WHERE context_included = 1` so excluded payloads are not
  deserialized on the `run_chat` path (`COUNT(*)` still supplies full `db_count`).

## Phase 2.3 (implemented) — `conversation_session` facade

- Unified write / working-set entry: [`conversation_session`](../../crates/pointer-core/src/conversation_session/mod.rs)
  (`append_missing`, `upsert_message`, `sync_ordered`, `prepare_lead_history`, …).
- Every `ConversationStore` transcript mutation bumps a per-conversation
  **generation** and clears the in-process working-set cache.
- `run_chat` begin uses `prepare_lead_history`: cache hit when generation matches
  (no DB working-set reload); otherwise incremental append or full
  `load_lead_working_messages`.
- UI paging / FTS / meta listing still read SQLite directly (not the working set).

## Still cloned (acceptable / later)

- `make_openai_messages_with_inject` still clones included rows into the filter/expand pipeline (needed for tool flatten).
- Background memory review and parallel `web_search` may snapshot `history` into an owned `Vec` / `Arc` for async lifetimes.
- Frontend **display slim** for old in-memory messages (phase 2.4) is deferred.
- Further sealing of remaining `ConversationStore` message writes in other crates.

## Related

- Extension hook contract: [`../developer/agent-extension-hooks.md`](../developer/agent-extension-hooks.md)
- Prompt assembly order: [`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)
- Persist / hydration: [`context-compression.md`](context-compression.md)
