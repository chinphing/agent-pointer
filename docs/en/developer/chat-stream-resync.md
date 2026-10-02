# Web chat stream weak-network reconciliation (SSE gap)

English | [简体中文](../../zh-CN/developer/chat-stream-resync.md)

## Problem

The web client receives `StreamEvent` over `GET /api/chat/:id/stream` (SSE).

Routing:

| Subscription | Events received |
| --- | --- |
| `global` | process-level events with no conversation id (console PTY, channel pairing), plus conversation events **belonging to the currently signed-in user** |
| a specific `conversationId` | **only** that conversation (denied by default; deltas with missing fields are no longer pushed to every connection) |

Events live only in the broadcast ring:

- When the client is slow / the network is weak, `tokio::broadcast` goes **Lagged**; frames already emitted (including `delta` / `done`) are dropped and **not replayed**
- After an SSE reconnect you likewise cannot get the events from the disconnected period

Symptoms:

1. The server has already persisted the full reply, but the UI shows it only after a refresh
2. The UI stays on "running" (`generating` is not cleared, or a tool / agentTrace is still `running`)
3. On a weak network the current conversation still shows as running after `done` is lost
4. **First message of a new conversation**: `onStream` fires `POST /api/chat` before the SSE subscription is attached; the broadcast has zero subscribers and drops frames, so the reply appears only after a refresh
5. **Background job occupancy**: the stop button / sidebar spinner / Composer bar all trust the JobSupervisor `jobs[]` (`background_jobs` plus the queue snapshot; the badge = `jobs.length`). `Done.backgroundRunningCount` clears it only at 0. It is **not** driven by `Done` alone, and **not** by the parent-message host row. After a process restart the JobSupervisor is empty while a persisted host row may still be `running` — the reconciliation snapshot is complete (not listed = 0), so never pour occupancy back from the host row.

The desktop client uses the Tauri event channel and has no such SSE ring; this reconciliation logic is harmless on desktop (`onGap` is a no-op and `waitForChatStreamReady` returns immediately).

## Current strategy

| Layer | Behaviour |
|----|------|
| Server `chat_stream` | broadcast buffer 4096; delivers per-conversation envelopes (see above); on `Lagged` logs a warn and sends `event: resync` to that SSE connection |
| Web `onStream` | **starts as soon as `chat.init` runs** (in parallel with loading the project / conversation list); resolves only after the first successful open; `waitForChatStreamReady` is false while disconnected. Calls `onGap(reason)` on `resync`, when the stream body ends, and on 502/504/error reconnects |
| Send gate | `dispatchChatTurn` does `await waitForChatStreamReady()` before `POST /api/chat`, so the first message of a new conversation does not drop frames while there are zero subscribers |
| Run-state reconciliation | `flags`: clear/set `generating` against the dispatcher, and reset background occupancy from the `backgroundJobs` snapshot (online, visibility, boot). When occupancy reaches zero while a host row is still `running`, align with the persisted handle first, then handle zombie rows (see `docs/ui/background-job-ui-consistency.md`) |
| **Message fetching** | **only** SSE-disconnect-style reasons take the full `catch_up`: `server_lagged` / `stream_ended*` / `stream_error` / `stream_gateway_error` (including the compatible `sse_gap`). In addition: under `flags` mode, clearing a conversation whose server run has ended while the UI is still `generating` also `force`-hydrates that conversation (covering the case where the first message never attached SSE) |
| catch_up hydration | `ensureMessagesLoaded({ force, silent })`, re-fetching the recent turn window (not a full load), without flashing the hydrating UI |
| Merge | during a forced hydration keep live streaming flags; take the more complete side of body/tools vs DB |
| Done chime | play while `generating` or while an active turn timing remains |

There is no longer any timed polling while generating; stuck run states rely on SSE gap / online / visibility.


## Related files

- `server/src/main.rs` — `chat_stream`
- `src/lib/web.ts` — `onStream` / `waitForChatStreamReady`
- `src/stores/chat.ts` — `dispatchChatTurn` / `syncRunStateFromDispatcherQueue` / `resyncAfterStreamGap`
