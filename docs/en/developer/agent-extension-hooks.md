# Agent extension hooks (Extension Hooks)

English | [简体中文](../../zh-CN/developer/agent-extension-hooks.md)

This document describes the plugin mechanism in **pointer-app** that corresponds to `python.helpers.extension` in the Python project **Pointer** (`PyProjects/pointer`): when extension points fire, how they are registered, and how they cooperate with agents such as Computer.

Implementation location: `crates/pointer-core/src/extensions/`. For the ordering of the **system assembly blocks, env, task board, and history messages** within HTTP in the main conversation (including the "before change" baseline notes), see **[`llm-prompt-assembly-order.md`](../../zh-CN/internals/llm-prompt-assembly-order.md)**.

---

## 1. Design goals

- **Align with Python's "extension point" concept**: insert async logic at fixed stages of the message loop, instead of hard-coding branches in `chat_service`.
- **Explicit registration**: unlike Python, the Rust side does **not** scan `extensions/<point>/` by directory; hooks are registered through the `register_*` methods of `ExtensionRegistry` (built-in hooks are registered in `AppState::new` via `register_builtin_extensions`).
- **Replaceable**: registering the same `override_key` again **overrides** the old hook, which makes customization or A/B implementations easy without adjusting registration order.

---

## 2. Correspondence with Python Pointer

| Python | Rust (pointer-core) |
|--------|----------------------|
| `Extension` base class + `async def execute(**kwargs)` | `async_trait`: `MessageLoopPromptsAfterHook` / `BeforeMainLlmCallHook` |
| `call_extensions("message_loop_prompts_after", …)` | `ExtensionRegistry::run_message_loop_prompts_after` |
| `call_extensions("before_main_llm_call", …)` | `ExtensionRegistry::run_before_main_llm_call` |
| After merging multiple paths, dedupe by **module file name**, **first occurrence wins** | Dedupe by `override_key`, **later registration overrides earlier** (see the difference below) |
| After dedupe, execute sorted lexicographically by **file name** | Execute sorted by `sort_key()` in **string lexicographic order** |
| `.py` files under `agents/computer/extensions/...` | Computer-specific hooks live in `agents/computer/extension_hooks/`; the general registry is in `extensions/` |

**Deduplication semantics difference (important)**

- **Python**: while walking paths, the implementation for the same "logical file name" that appears **first** takes effect (higher-priority paths override lower-priority ones).
- **Rust**: the **last** `register_*` for the same `override_key` takes effect, which better matches the explicit-API habit of "later installs override defaults". To mimic Python's "built-ins are read-only and cannot be overridden", do not register the same `override_key` again.

---

## 3. Core types

### 3.1 `ExtensionRegistry`

- Hangs off **`AppState`**: `AppState.extensions: Arc<ExtensionRegistry>`.
- Internally maintains two lists (conceptually corresponding to the two extension points):
  - `message_loop_prompts_after`
  - `before_main_llm_call`
- At runtime it snapshots the list, sorts it by `sort_key`, and calls `execute` in order; errors propagate upward through `anyhow::Result` and can abort the current request.
- After each hook executes it writes `phase_timing: phase=message_loop_prompts_after_hook` / `before_main_llm_call_hook` (including `key=`); the total is written as `*_total`. Over 1 second is a `warn`.

Other pre-stream stages (login refresh, balance, workspace, attachments, tool schema) also go through `phase_timing:`; see [`workspace-root.md`](workspace-root.md#agentsmd-vs-session-workspace).

### 3.2 `override_key` and `sort_key`

Every hook must implement:

| Method | Purpose |
|------|------|
| `override_key()` | Stable identity, equivalent to the **source file name** used for deduplication on the Python side (e.g. `_10_computer_screen_inject`). Within the same extension point, only the **last registration** for the same key is kept. |
| `sort_key()` | **Execution order**; the recommended prefix style matches Python: `_10_…`, `_20_…`, `_75_…`, compared as strings. |

The two may be identical (as they currently are for the Computer screen injection).

### 3.3 Context objects (field-level description)

**`MessageLoopPromptsAfterContext`**

| Field | Meaning |
|------|------|
| `computer_state` | The globally unique Computer runtime state: the screenshot/annotation client, the shared `VisionState` (index → pixels), the action executor, etc. |
| `lead_agent_profile` | The profile of the agent being conversed with **in this round**. In single-agent mode it is the main lead's profile; in a sub-agent round it is the profile **defined for that sub-agent** (for example `Computer` / `Coder`). Hooks use it to decide whether to be a no-op (e.g. inject the screen only for `Computer`). |
| `base_messages` | **Read-only**: this round's authoritative transcript (the main conversation `history` or the sub-agent `local_history`). Hooks **must not** modify it or `push` to it. |
| `injected_tail` | **Mutable**: the ephemeral lines used only for this round's API request. Hooks **append** here (e.g. `[CUR_SCREEN]` / the task-board user block); it is **not** written back to the persisted `history`. |
| `conversation_id` | The current conversation id (consistent with the frontend/Tauri stream). |
| `stream` | Optional `ChatStreamSender`; when present, hooks may send **`StreamEvent::UiToast`** (a UI banner reminder only — **not** written to the chat record and **not** included in the model payload). |
| `round_assistant_message_id` | Optional; this round's assistant message id (consistent with the main loop's `MessageStart`, or in a sub-agent round the **parent** assistant bubble id). Injection uses it to send **`StreamEvent::AssistantRoundScreen`**. |
| `round_screen_dump_prefix` | Optional; the file name prefix used when dumping a debug image to disk; defaults to the same value as `round_assistant_message_id`. Each sub-agent iteration uses its own id, to avoid mixing with the parent message id. |
| `workspace_root` | This round's conversation workspace (after `ensure_workspace_at_run_start`). Hooks such as plugin rules may read it; **`AGENTS.md` is no longer injected through this extension point** (see [`workspace-root.md`](workspace-root.md)). |

**`StreamEvent::AssistantRoundScreen`** carries only `annotatedRelPath` (relative to `PointerApp/computer-captures/` under the same application data root as settings/skills; written to disk **only in `debugMenusEnabled` debug mode**), avoiding pushing a large image's base64 into the stream and memory; the UI reads from disk when the user clicks to preview: **Tauri** uses `preview_computer_round_screen`, **pointer-server** uses `GET /api/computer/round-screen-preview?relPath=…` (corresponding to `GET /api/computer/annotated-preview` on Tauri's `preview_computer_annotated_screen`). **On both desktop and server startup** the same **7-day** screenshot directory cleanup runs (`capture_debug::CAPTURE_RETENTION_DAYS`); if anything was deleted, a **`UiToast`** is sent to the event bus (`conversationId` empty = global "expired screenshots cleaned up").

**`BeforeMainLlmCallContext`**

| Field | Meaning |
|------|------|
| `computer_state` | Same as above. |
| `lead_agent_profile` | Same as above; consistent with the "speaker" of this round's `stream_chat`. |
| `system_prompts_dynamic` | **Mutable**: this round's **dynamic** section (commonly `[LOCKED GOAL]` and similar). **cacheable** already contains public communication, Agent/Skills, the tool appendix, and **`[Environment]`** before the hook runs. For details see **[`llm-prompt-assembly-order.md`](../../zh-CN/internals/llm-prompt-assembly-order.md)**. |
| `conversation_id` | The main conversation id (streaming/UI); under a sub-agent it is still the **parent conversation** id. |
| `task_board_store` | `Arc<TaskBoardStore>`, for built-in or custom hooks to read the task board. |
| `task_board_store_key` | The key passed to `TaskBoardStore::snapshot_for_prompt`: the main conversation uses `conversation_id`; a sub-agent uses the composite key from `sub_agent_task_board_store_key(...)`. |

---

## 4. Injection nodes and timeline (a single model request round)

The following describes, in **chronological order**, what happens before a "model call". Code paths: `run_single_agent_loop` in `single_agent.rs`; each round's prompt assembly is in `single_agent_prompt.rs`; streaming receive is in `single_agent_stream.rs` (sharing `agent_stream_round.rs` internally); post-stream decisions (empty tool / envelope retry) are in `agent_post_stream.rs` (Lead goes through the thin wrapper `single_agent_post_stream.rs`); tool landing is in `agent_tool_pass.rs` (Lead goes through the thin wrapper `single_agent_tools.rs`). The sub-loop is in `sub_agent.rs` + `sub_agent_prompt.rs` + `sub_agent_stream.rs`, **isomorphic** with Lead and sharing the shared modules above; the difference is the initial `messages` snapshot and how format retries are delivered — see section 5.

### 4.1 Timeline (coarse-grained)

Within one iteration round, the order is fixed:

1. **Enter this round** — check cancellation and tool budget; generate this round's `assistant_id` (used for UI streaming).
2. **`MessageStart` (and `AgentStep` for sub-agents)** — create the frontend assistant bubble first, then run injection, so this round's screenshot event can be bound to that `message_id`.
3. **Prepare the Provider** — create `OpenAIProvider` and the channel; no HTTP sent yet.
4. **Prepare API input** — borrow the base history read-only (single-agent: `history`; sub-agent: `local_history`), create an empty `injected_tail`, and fill in `round_assistant_message_id`.
5. **`message_loop_prompts_after`** — `run_message_loop_prompts_after`: only appends ephemeral lines to `injected_tail` (e.g. screen injection).
6. **Assemble system (cacheable)** — public communication, Agent/Skills, tool appendix, then `push_env_to_cacheable` (`[Environment]`), `[USER RULES]`, and **`# Project Context`** (the `AGENTS.md` chain).
7. **`before_main_llm_call`** — hooks append to **`system_prompts_dynamic`** (e.g. `[LOCKED GOAL]`).
8. **Build the wire → `stream_chat_wired`** — `build_stream_chat_wire(base, injected_tail, SystemPromptSections)` → `make_openai_messages_with_inject` (for Qwen see **[`qwen-context-cache.md`](../../zh-CN/llm/qwen-context-cache.md)**); the HTTP task holds only the wire JSON, no longer a full `history` clone. The Computer **full wall-clock time** is in the **`[CUR_SCREEN]`** `user` message (`screen_inject.rs`).

Key point: the task board has moved to an end-of-turn user injection block from `message_loop_prompts_after` and no longer occupies system cacheable/dynamic. For long-conversation peak memory see **[`../internals/long-chat-memory.md`](../../zh-CN/internals/long-chat-memory.md)**.

### 4.2 Single-agent: the message view at injection time

- **Base history**: the multi-round messages already persisted in the conversation (borrowed read-only; the structure before user / assistant / tool rounds are flattened is handled by the subsequent `make_openai_messages_with_inject`).
- **Injection append**: for example, Computer appends a `User` message to the **end** of `injected_tail` that is used only for this request (**`Local wall-clock at capture:`** includes the full date and time, followed by **`[CUR_SCREEN]`** and optional `images_base64`).
- **Not persisted**: after this round ends, the persisted `history` still only appends **real** assistant / tool messages as before; ephemeral injections are **not** written into the conversation store.

### 4.3 Multiple model calls within one round (tool loop)

After the user sends one message, several rounds of "model → tool → model" may follow. **Every round** of a new model request repeats steps 4–8 above:

- Every round borrows the current `history` read-only (already containing the previous round's assistant and tool results) and creates a fresh `injected_tail` for the round.
- Every round runs `message_loop_prompts_after` / `before_main_llm_call` again (the task board summary is refreshed in the end-of-turn user block when there is content).  
Therefore Computer **re-captures and annotates the screen every round** (consistent with Python's per-round inject).

### 4.4 Sequence diagram (consistent with 4.1)

```mermaid
sequenceDiagram
    participant Loop as Message loop (per tool cycle round)
    participant Hist as Persisted history
    participant Tail as injected_tail
    participant Ext1 as message_loop_prompts_after
    participant Sys as Assembled system prompts
    participant Ext2 as before_main_llm_call
    participant Wire as build_stream_chat_wire
    participant LLM as stream_chat_wired

    Loop->>Hist: borrow the current history read-only
    Loop->>Tail: create an empty injected_tail
    Loop->>Ext1: run_message_loop_prompts_after(ctx)
    Note over Ext1,Tail: append [CUR_SCREEN] etc. to tail
    Loop->>Sys: cacheable = public / Agent / tools / Environment
    Loop->>Ext2: run_before_main_llm_call(ctx)
    Note over Ext2,Sys: dynamic += [LOCKED GOAL] etc.
    Loop->>Wire: base + injected_tail + SystemPromptSections
    Loop->>LLM: send HTTP holding only the wire JSON
```

---

## 5. Sub-agents: how context is passed in, and whether it is independent

### 5.1 The sub-agent's "conversation context" — an **independent mini session**

- A sub-agent does **not** use the main conversation's `history` as model input.
- The initial `local_history` contains **only one** short stub `User` message; **`goal`** / **`context`** are written into system dynamic by the host (**Assigned task**).
- If the task has `dependsOn`, the implementation writes the dependency tasks' output summaries into **`context`** (`[Prior task outputs]`) and does **not** splice them into `goal`.
- In the sub-agent's own multi-round tool loop, it only accumulates this round's assistant, tool, etc. on `local_history`, **isolated** from the main `history`.

On the system-prompt side the sub-agent is isomorphic with the main round: **cacheable** is assembled by the shared function `push_agent_role_cacheable_prompts` (`COMMUNICATION_PUBLIC` + the Computer **tier** slice, or the non-Computer `system_prompt`), then appends **sub_agent_header** (for Computer only a short handoff note, without the baked `agent.system_prompt()`), skills, the tool appendix, Environment, and **`# Project Context`** (the `AGENTS.md` chain); the task board is supplied by the end-of-turn user injection (when there is content).

**Conclusion (conversation semantics)**: a sub-agent **is independent in the sense of the message list**; it only "sees" the task description + (optionally) prior task summaries + the history produced by its own multi-round tools.

### 5.2 Sub-agents and extension hooks

- Conversation initialization and each round's prompt assembly are in **`sub_agent_prompt.rs`** (`init_sub_agent_session` / `prepare_sub_agent_round_prompts`); streaming receive is in **`sub_agent_stream.rs`** (sharing **`agent_stream_round.rs`** internally); post-stream decisions and tool execution share **`agent_post_stream.rs`** / **`agent_tool_pass.rs`** respectively (isomorphic with Lead; see §5.2.1, differences between modules shared by Lead and sub-agents (behavior unchanged)).
- Before every model request in every sub-agent round, the same steps run:
  - borrow `local_history` read-only and create `injected_tail`
  - `run_message_loop_prompts_after` (`lead_agent_profile = def.profile`; for example, when the sub-agent is `computer` the screen is still injected)
  - **cacheable** = `push_agent_role_cacheable_prompts` + the sub-agent `session_extras` + **`tools_system_appendix`** + Environment
  - `run_before_main_llm_call` (dynamic appends other system dynamic blocks; `task_board_store_key` is still used for task-board read/write/injection)
  - `build_stream_chat_wire(local_history, injected_tail, …)` then `stream_chat_wired`
- The **`ExtensionRegistry` used is the same as for single-agent** (`AppState.extensions`), **not** one per sub-agent.

#### 5.2.1 Differences in modules shared by Lead and sub-agents (behavior unchanged)

| Stage | Lead (`single_agent.rs`) | Sub (`sub_agent.rs`) |
|------|---------------------------|------------------------|
| Streaming UI | `ContentDeltaMode::LeadMessage` (`RawContentDelta` + `Delta`) | `ContentDeltaMode::SubAgentTrace` (`emit_agent_content_delta`) |
| Format-retry user line | `InjectedUserMessage` + main `history` | Push to `local_history` only |
| Tool-budget-exhausted copy / compression | `compress_for_session = true` | `compress_for_session = false` |
| Termination shape | `Ok(())` | `AgentRunResult` (`response` or natural language with no tool) |
| Tool pass | May delegate via `run_subagent` | Hard-rejects `run_subagent`, validates against `allowed_tools` |

### 5.3 Shared resources that are "not independent" from the main conversation (important)

The following are **globally shared** within the process, and affect each other when the main agent and sub-agents use them **serially or interleaved**:

| Resource | Description |
|------|------|
| `AppState.computer_state` | The same `VisionState`, the same annotation client and executor. If a sub-agent `Computer` runs screen injection it updates the **same** `index_map` / `screen_bbox`. If the main conversation also uses Computer, or there are several Computer subtasks in a row, a later round sees the visual state written by the previous round, unless it is cleared or isolated at the business layer. |
| `AppState.tools` / `skills` / `agents` | Global registries, read-only for configuration only. |
| Tool budget `SessionToolBudget` | **Outer** orchestration (the single-agent main loop each time a `run_subagent` completes) and the **inner** sub-agent tool loop are **counted separately**: each `run_sub_agent` uses a **new** inner budget instance (the cap comes from `maxSubAgentToolRounds`); the outer loop calls `record_tool_cycle` once at the end of a round that includes tool execution (including the round containing `run_subagent`). |

**Conclusion (runtime)**: a sub-agent's **conversation context is independent**, but **global state with side effects such as Computer is not**; when designing extensions or parallel subtasks, consider the semantics of `VisionState` and the budget.

---

## 6. Correspondence with Python `prepare_prompt` (summary)

- Python: calls `message_loop_prompts_after` after setting `loop_data.system` and `loop_data.history_output`.
- Rust: first runs extensions against the **API `messages`**, then assembles the **system-side `prompts_clone`** and calls `stream_chat`. To align with Python's "extensions can read the full loop_data.system", you must adjust the order or extend `Context`.

---

## 7. Built-in hooks at a glance

| override_key / sort_key | Extension point | File | Behavior summary |
|-------------------------|--------|------|----------|
| `_10_computer_screen_inject` | `message_loop_prompts_after` | `agents/computer/extension_hooks/screen_inject.rs` | When `lead_agent_profile == Computer`: `capture_and_annotate` and append a temporary user message with PNG base64 to `messages`; on failure append a plain-text explanation. |
| `_99_common_user_dynamic_inject` | `message_loop_prompts_after` | `extensions/common_user_dynamic_inject_hook.rs` | Appends a user injection block to the end of this round's `messages`: the `task_board` Markdown (main conversation or subtask key; when there is content or an init hint). |

A custom hook may **replace** `_99_common_user_dynamic_inject` with the same `override_key` to change the format or turn injection off.

---

## 8. Registration

### 8.1 Default (application startup)

The approximate order in `AppState::new` is:

1. `ExtensionRegistry::new()`
2. `extensions::register_builtin_extensions(&mut registry)` (including `_10_computer_screen_inject`, `_99_common_user_dynamic_inject`, etc.)
3. `Arc::new(registry)` stored in `AppState.extensions`

### 8.2 Adding or overriding hooks

Given a mutable `ExtensionRegistry` (for example in a custom `AppState` construction):

```rust
use std::sync::Arc;
use pointer_core::extensions::{ExtensionRegistry, MessageLoopPromptsAfterHook, /* … */};

let mut registry = ExtensionRegistry::new();
pointer_core::extensions::register_builtin_extensions(&mut registry);

// Override the Computer screen injection (same override_key)
registry.register_message_loop_prompts_after(Arc::new(MyComputerScreenHook));

let extensions = Arc::new(registry);
```

When implementing a new hook you must use `async_trait::async_trait` and guarantee `Send + Sync` (usually a `struct` with no interior mutable shared state, or fields inside an `Arc`).

### 8.3 Helper functions

- `extensions::new_extension_message_id(prefix)` — generates a temporary message id (e.g. `screen_inject_*`).
- `extensions::now_ms()` — the message `created_at` timestamp, consistent with other chat paths.

---

## 9. Checklist for implementing a new hook

1. Choose the extension point: `message_loop_prompts_after` or `before_main_llm_call`.
2. Choose a **globally unique** `override_key` for the hook (if you do not want to replace the built-in Computer injection, do not use `_10_computer_screen_inject`).
3. Set `sort_key` so that the relative order with other hooks in the same extension point is as expected.
4. Inside `execute`, decide for yourself whether to be a no-op based on `AgentProfile` and so on (consistent with each Python extension checking `profile`).
5. For `MessageLoopPromptsAfter`: do **not** assume `messages` is persisted to the conversation store; it only affects this HTTP request payload.
6. When you need more context, prefer extending the `*Context` struct and passing it in from `chat_service`, rather than pulling global singletons inside the hook.
7. If the hook depends on **Computer global state**, also consider the `VisionState` semantics when **sub-agents** and the **main conversation** execute interleaved (see section 5.3).

---

## 10. Limitations and future directions

- **No dynamic disk scanning**: loading `.so` or scripts from `usr/extensions` at runtime is not supported; extensions are all compiled into `pointer-core` or registered by an upper-layer crate.
- **Number of extension points**: currently only the two points strongly related to the Computer pipeline are implemented; to align with Python's `tool_execute_before`, `response_stream_chunk`, etc., you would need new traits, `ExtensionRegistry` fields, and explicit `run_*` calls on the `provider` / tool-execution paths.
- **Context fields**: `BeforeMainLlmCallContext` contains the mutable **`system_prompts_dynamic`** plus `task_board_store` / `task_board_store_key`; a custom hook can replace the built-in task-board snapshot behavior with the same `override_key`.
- **Aligning order with Python**: an optional refactor to "assemble system first, then run `message_loop_prompts_after`" so hooks can read the full system text.

---

## 11. Related docs and code

- The Computer data flow in the implementation plan: [`computer-use-implementation-plan.md`](../../zh-CN/design/computer-use-implementation-plan.md)
- Registry and traits: `crates/pointer-core/src/extensions/mod.rs`
- Computer screen injection: `crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs` (aggregated and registered by `extension_hooks/mod.rs`)
- Call sites: `session_inner.rs`, `single_agent.rs`, `single_agent_prompt.rs`, `single_agent_stream.rs`, `agent_stream_round.rs`, `agent_post_stream.rs`, `agent_tool_pass.rs`, `sub_agent.rs`, `sub_agent_prompt.rs`, `sub_agent_stream.rs` (search for `run_message_loop_prompts_after`, `run_before_main_llm_call`, `run_sub_agent`)
