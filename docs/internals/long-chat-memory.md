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
  hydrated `sendChat`: only non-persisted **lead** rows plus
  `listUnpersistedRows` (not a full store flatten) are deep-cloned.
  Hydrated `sendChat` never includes scoped rows (backend `persist_sub_message`
  already wrote them). Empty incremental does **not** fall back to a full clone.

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

## Do not load the full transcript to start a turn

`run_chat` replaces the incoming vec with the lead working set
(`context_included = 1`) inside `prepare_lead_history`. Callers that only
need to append one new user row — idle job push, cron — pass that row alone.
Loading `load_messages` first deserializes every soft-excluded and scoped
payload (tens of thousands of rows on a long session) and the process keeps
that allocator arena after the vec is dropped.

Patching one host tool call or one tool result uses `load_message` /
`load_tool_message_by_call_id`. Follow-up goal lookup uses the tool-content
and assistant tool-call lookups. Those paths must not call `load_messages`.

The same rule covers the remaining full-transcript readers:

- **Turn id for file baselines.** `run_chat` pins the latest real lead user
  message id once (`remember_active_turn_id`). `file_write` / `file_edit`
  reuse that id for the rest of the turn, including sub-agent writes. A miss
  (restart, or a write before the pin) reads `message_id` + `content` from
  the tail (`role=user`, `is_scoped=0`, skip synthetic content) and then pins
  it. Do not use `is_system_generated` for this anchor, and do not call
  `load_messages`.
- **IM and webhook append.** Dispatch only the new user message. Webhook
  full-history override (more than one message, or any assistant/tool row)
  still passes the request messages and does not read the transcript.
- **Last assistant reply.** IM pages assistant payloads until `rawContent`
  or `content` is non-empty. Webhook polling reads the newest non-empty
  assistant `content` column.
- **Attachments.** Find by id or media ref scans user payloads that contain
  the needle, newest first, and stops on the first confirmed hit. Candidate
  lists read recent user rows that have attachments until a few are collected.
- **Legacy full loads** (`load_conversations`, unpaged
  `load_conversation_messages`) stay available for external callers and log
  the row count. Do not send the UI down those paths.

## Still cloned (acceptable / later)

- `make_openai_messages_with_inject` still clones included rows into the filter/expand pipeline (needed for tool flatten).
- Background memory review and parallel `web_search` may snapshot `history` into an owned `Vec` / `Arc` for async lifetimes.
- Frontend **display slim** for old in-memory messages (phase 2.4) is deferred.
- `ConversationStore` fine-grained message writes are `pub(crate)` (sealed for
  external crates); legacy full-transcript `save_all` was removed from
  production (test-only convenience, `#[cfg(test)]`).

## Related

- Extension hook contract: [`../developer/agent-extension-hooks.md`](../developer/agent-extension-hooks.md)
- Prompt assembly order: [`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)
- Persist / hydration: [`context-compression.md`](context-compression.md)
