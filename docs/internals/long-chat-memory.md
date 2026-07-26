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

## Still cloned (acceptable / later)

- `make_openai_messages_with_inject` still clones included rows into the filter/expand pipeline (needed for tool flatten).
- Background memory review and parallel `web_search` may snapshot `history` into an owned `Vec` / `Arc` for async lifetimes.
- Frontend **display slim** for old in-memory messages (phase 2.2) is deferred.

## Related

- Extension hook contract: [`../developer/agent-extension-hooks.md`](../developer/agent-extension-hooks.md)
- Prompt assembly order: [`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)
- Persist / hydration: [`context-compression.md`](context-compression.md)
