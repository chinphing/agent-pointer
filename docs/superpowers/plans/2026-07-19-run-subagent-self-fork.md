# `run_subagent` Self-Fork Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `general` and `coder` to emit multiple `run_subagent(agentId="self")` calls that execute concurrently as isolated leaf forks, while preserving existing cross-agent delegation.

**Architecture:** Extend the current single-call protocol with the reserved target `self`. The batch planner groups only self-targeted calls into a dedicated parallel wave; each fork executes against an immutable current-agent snapshot and accumulates trace/usage/result state locally, then the parent commits outcomes in original tool-call order. Tool inheritance is controlled by per-tool registry metadata that defaults to inheritable and explicitly denies global, recursive, durable, final-reply, and desktop tools.

**Tech Stack:** Rust, Tokio, Tauri shared `pointer-core`, serde JSON tool schemas, SQLite-backed transcript/task-board stores.

## Global Constraints

- Do not add a new model-facing tool.
- Do not add a `tasks[]` argument.
- Existing `run_subagent(agentId=<worker>)` behavior remains compatible and serial.
- Self-fork children are leaves and cannot inherit `run_subagent`.
- `maxParallelSubAgents` is the runtime concurrency limit.
- Results are committed in original assistant `tool_calls` order.
- APP and pointer-server must use the same implementation.
- Windows, macOS, and Linux share the same scheduler behavior.
- Do not commit changes unless the user explicitly requests a commit.

---

### Task 1: Add per-tool inheritance metadata

**Files:**
- Modify: `crates/pointer-core/src/tools/mod.rs`
- Modify: `crates/pointer-core/src/tools/run_subagent.rs`
- Modify: `crates/pointer-core/src/memory/tool.rs`
- Modify: `crates/pointer-core/src/tools/skill.rs`
- Modify: `crates/pointer-core/src/tools/cron_job/mod.rs`
- Modify: `crates/pointer-core/src/tools/media_generate.rs`
- Modify: `crates/pointer-core/src/task_board/tool.rs`
- Modify: `crates/pointer-core/src/agents/computer/tools/mod.rs`
- Test: `crates/pointer-core/src/tools/mod.rs`

**Interfaces:**
- Produces: `ToolEntry::inherit_to_subagent: Option<bool>`
- Produces: `ToolEntry::with_subagent_inheritance(bool) -> Self`
- Produces: `ToolRegistry::is_inheritable_to_subagent(&str) -> bool`

- [ ] **Step 1: Add failing metadata tests**

Add tests proving new tools default to inheritable and can opt out:

```rust
#[test]
fn tools_default_to_subagent_inheritable() {
    let entry = test_entry("file_read");
    assert!(entry.inherit_to_subagent.unwrap_or(true));
}

#[test]
fn tools_can_disable_subagent_inheritance() {
    let entry = test_entry("run_subagent").with_subagent_inheritance(false);
    assert_eq!(entry.inherit_to_subagent, Some(false));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core tools:: --lib
```

Expected: compile failure because the metadata and builder do not exist.

- [ ] **Step 3: Implement metadata and registry query**

Add to `ToolEntry`:

```rust
pub inherit_to_subagent: Option<bool>,
```

Initialize it to `None` in `new_inner`, add:

```rust
pub fn with_subagent_inheritance(mut self, inherit: bool) -> Self {
    self.inherit_to_subagent = Some(inherit);
    self
}
```

Add to `ToolRegistry`:

```rust
pub fn is_inheritable_to_subagent(&self, name: &str) -> bool {
    self.get(name)
        .map(|entry| entry.inherit_to_subagent.unwrap_or(true))
        .unwrap_or(false)
}
```

- [ ] **Step 4: Mark explicit deny tools**

Apply `.with_subagent_inheritance(false)` to:

```text
run_subagent
memory
skill_import
cron_job
image_generate
video_generate
task_board_abandon
all Computer / clipboard / app-access tools
```

Do not mark file, terminal, read, search, media-understand, or child task-board tools false.

- [ ] **Step 5: Run metadata tests**

Run the Task 1 command again.

Expected: all tool tests pass.

---

### Task 2: Parse and validate the reserved `self` target

**Files:**
- Modify: `crates/pointer-core/src/tools/run_subagent.rs`
- Modify: `crates/pointer-core/src/tools/prompts/run_subagent.md`
- Test: `crates/pointer-core/src/tools/run_subagent.rs`

**Interfaces:**
- Produces: `RunSubagentArgs::is_self_fork() -> bool`
- Produces: `validate_run_subagent_target` behavior that bypasses `allowAgents` only for `self`
- Preserves: normal target registry/role/enabled checks

- [ ] **Step 1: Write failing parsing and validation tests**

```rust
#[test]
fn self_target_is_a_self_fork() {
    let args = parse_run_subagent_args(&json!({
        "agentId": "self",
        "goal": "What: inspect backend\nDone when: report findings"
    })).unwrap();
    assert!(args.is_self_fork());
}

#[test]
fn self_target_does_not_require_allow_agents() {
    let resolved = validate_run_subagent_target(
        &registry(),
        &[],
        "self",
    ).unwrap();
    assert!(matches!(resolved, RunSubagentTarget::SelfFork));
}
```

Also retain tests proving unknown and disallowed normal ids fail.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core run_subagent --lib
```

Expected: compile failure for `is_self_fork` / `RunSubagentTarget`.

- [ ] **Step 3: Add explicit target enum**

Introduce:

```rust
pub enum RunSubagentTarget {
    SelfFork,
    Registered(AgentDef),
}
```

Change target validation to return `SelfFork` for the exact reserved id
`self`; all other values continue through current allow-list and registry
validation.

Self target does not require coder `workspaceRoot`; it inherits the current
workspace. Existing `agentId="coder"` still requires it.

- [ ] **Step 4: Update model-facing documentation**

Document:

- one call represents one task;
- multiple independent self calls in the same assistant turn may run concurrently;
- self forks are leaf workers;
- dependent tasks and overlapping writes must not be forked concurrently.

- [ ] **Step 5: Run focused tests**

Expected: all `run_subagent` tests pass.

---

### Task 3: Build an immutable self-fork execution snapshot

**Files:**
- Create: `crates/pointer-core/src/chat_service/self_fork.rs`
- Modify: `crates/pointer-core/src/chat_service/mod.rs`
- Modify: `crates/pointer-core/src/chat_service/context.rs`
- Modify: `crates/pointer-core/src/chat_service/sub_agent_prompt.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_allowlist.rs`
- Test: `crates/pointer-core/src/chat_service/self_fork.rs`

**Interfaces:**
- Produces:

```rust
pub struct SelfForkSnapshot {
    pub def: AgentDef,
    pub system_prompt: String,
    pub skill_ids: Vec<String>,
    pub skill_prompts: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub workspace_root: String,
}
```

- Produces:

```rust
pub fn build_self_fork_snapshot(
    current_def: &AgentDef,
    current_system_prompt: &str,
    current_skill_ids: &[String],
    current_allowed_tools: &[String],
    workspace_root: &str,
    registry: &ToolRegistry,
) -> SelfForkSnapshot
```

- [ ] **Step 1: Write failing snapshot tests**

Test that:

- profile/id/prompt/workspace are retained;
- inherited tools preserve order;
- unregistered tools are removed;
- `inherit_to_subagent=false` tools are removed;
- `run_subagent` is absent;
- file write/edit and terminal remain.

- [ ] **Step 2: Run focused tests and verify failure**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core self_fork --lib
```

- [ ] **Step 3: Implement snapshot construction**

Filter with:

```rust
allowed_tools.retain(|name| registry.is_inheritable_to_subagent(name));
```

Use the active parent definition and already-resolved tool/skill state rather
than re-reading `"self"` from `AgentRegistry`.

- [ ] **Step 4: Extend sub-agent session initialization**

Allow `init_sub_agent_session` to consume either:

```rust
SubAgentDefinitionSource::Registered(&AgentTask)
SubAgentDefinitionSource::Snapshot(&SelfForkSnapshot)
```

Both paths must create a fresh local history and unique
`AgentInstanceScope`; the snapshot path must not add `run_subagent`.

- [ ] **Step 5: Run self-fork and sub-agent tests**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core self_fork sub_agent --lib
```

Expected: tests pass.

---

### Task 4: Separate child execution from parent-state commit

**Files:**
- Modify: `crates/pointer-core/src/chat_service/run_subagent_delegation.rs`
- Modify: `crates/pointer-core/src/chat_service/sub_agent.rs`
- Modify: `crates/pointer-core/src/chat_service/sub_message.rs`
- Modify: `crates/pointer-core/src/chat_service/emit.rs`
- Modify: `crates/pointer-core/src/chat_service/context.rs`
- Test: `crates/pointer-core/src/chat_service/run_subagent_delegation.rs`

**Interfaces:**
- Produces:

```rust
pub struct PreparedSubagentOutcome {
    pub tool_call_id: String,
    pub task_id: String,
    pub trace: AgentTrace,
    pub usage: ConversationLlmStats,
    pub exec: ToolExecResult,
}
```

- Produces:

```rust
pub async fn execute_self_fork(
    input: SelfForkExecutionInput<'_>,
) -> PreparedSubagentOutcome
```

- Produces:

```rust
pub fn commit_subagent_outcome(
    parent: &mut SubagentCommitContext<'_>,
    outcome: PreparedSubagentOutcome,
)
```

- [ ] **Step 1: Write failing isolation tests**

Verify two prepared outcomes can be created without mutating:

- parent history;
- parent `agent_trace`;
- parent token stats.

Verify commits merge trace and usage only when called.

- [ ] **Step 2: Run tests and verify failure**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core run_subagent_delegation --lib
```

- [ ] **Step 3: Extract current registered-agent behavior**

Keep the serial registered-agent path behavior unchanged, but move common child
construction, result serialization, failure board cleanup, and trace creation
into functions that return owned values.

- [ ] **Step 4: Implement self-fork execution**

Each fork must own:

- child task and task id;
- child trace buffer;
- child usage buffer;
- child task-board key;
- provider clone with inherited workspace;
- fresh local history.

It may publish scoped child stream events, but it must not borrow parent mutable
history/trace/stats across `.await`.

- [ ] **Step 5: Implement ordered parent commit**

Commit after execution using owned outcomes. Ensure every tool call gets a
success, failure, or cancelled result.

- [ ] **Step 6: Run delegation tests**

Expected: registered delegation tests and new isolation tests pass.

---

### Task 5: Schedule same-turn self calls concurrently

**Files:**
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/batch.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/mod.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/mod.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/subagent.rs`
- Modify: `crates/pointer-core/src/tools/parallel.rs`
- Test: `crates/pointer-core/src/chat_service/agent_tool_pass/batch.rs`
- Test: `crates/pointer-core/src/chat_service/agent_tool_pass/mod.rs`

**Interfaces:**
- Produces: `ToolWave::ParallelSelfFork(Vec<usize>)`
- Consumes: `execute_self_fork` and `commit_subagent_outcome`
- Consumes: `ParallelLimits.max_parallel_sub_agents`

- [ ] **Step 1: Add failing batch-plan tests**

Cover:

```text
self + self                 => one ParallelSelfFork wave
self + registered target    => serial boundary
registered + registered     => serial waves
self + computer             => serial boundary
force_serial=true           => serial waves
```

Determine self calls by parsed `agentId == "self"`, not by tool id alone.

- [ ] **Step 2: Run batch tests and verify failure**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core agent_tool_pass::batch --lib
```

- [ ] **Step 3: Add dedicated self-fork wave**

Do not route self forks through the generic parallel registry invocation,
which intentionally lacks mutable sub-agent context.

The planner groups consecutive self calls up to
`max_parallel_sub_agents`; ordinary subagent calls still flush the current wave
and become serial.

- [ ] **Step 4: Add the sub-agent semaphore**

Create:

```rust
let subagent_sem = Arc::new(Semaphore::new(
    parallel_limits.max_parallel_sub_agents.max(1),
));
```

Acquire one permit per self fork. Log wait time, task id, fork id, and limit.

- [ ] **Step 5: Execute concurrently and commit in index order**

Use `FuturesUnordered` for execution, store owned outcomes keyed by prepared
index, then sort/iterate by original index before `apply_one_outcome`.

Do not commit in completion order.

- [ ] **Step 6: Add cancellation and sibling-failure tests**

Verify:

- one failed child does not cancel another;
- parent cancellation stops pending/running children;
- limit 1 serializes execution;
- limit 2 permits overlap;
- output order remains input order.

- [ ] **Step 7: Run tool-pass tests**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core agent_tool_pass --lib
```

Expected: all tool-pass tests pass.

---

### Task 6: Isolate inherited shared-state tools

**Files:**
- Modify: `crates/pointer-core/src/chat_service/app_state.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/terminal.rs`
- Modify: `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/mod.rs`
- Modify: `crates/pointer-core/src/tools/file/mod.rs`
- Modify: file write/edit implementation files under `crates/pointer-core/src/tools/file/`
- Modify: `crates/pointer-core/src/task_board/inject.rs`
- Modify: `crates/pointer-core/src/task_board/coordination/parent_child.rs`
- Test: corresponding Rust modules

**Interfaces:**
- Produces: per-fork operation key:

```rust
pub struct ToolExecutionScope {
    pub conversation_id: String,
    pub agent_instance_id: Option<String>,
    pub tool_call_id: String,
}
```

- Produces: canonical path write-lock manager stored on `AppState`

- [ ] **Step 1: Add failing terminal routing tests**

Create two execution scopes under one conversation and assert their pending
input, output trace, and abort keys do not collide.

- [ ] **Step 2: Implement per-fork terminal keys**

Key terminal run/input state by agent instance plus tool call. Preserve
conversation-wide parent cancel by iterating all keys with the conversation
prefix.

- [ ] **Step 3: Add failing file-lock tests**

Verify two async writes to the same canonical path cannot overlap, while
different paths can.

- [ ] **Step 4: Implement canonical path locks**

Use a process-level map of canonical/normalized absolute path to Tokio mutex.
Acquire for write/edit duration and log non-zero waits.

- [ ] **Step 5: Add task-board isolation tests**

Verify unique task ids produce unique child keys and inherited task-board calls
receive only their child store key. Verify `task_board_abandon` is absent.

- [ ] **Step 6: Run focused tests**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core terminal file task_board --lib
```

Expected: focused tests pass.

---

### Task 7: Remove `general-worker` and teach General/Coder self-fork policy

**Files:**
- Delete: `crates/pointer-core/src/agents/general-worker/AGENT.md`
- Delete: other files under `crates/pointer-core/src/agents/general-worker/`
- Modify: `crates/pointer-core/src/agents/general/AGENT.md`
- Modify: `crates/pointer-core/src/agents/coder/AGENT.md`
- Modify: `crates/pointer-core/src/agents/coder/COMMUNICATION.md`
- Modify: agent registration/tests under `crates/pointer-core/src/agents/mod.rs`
- Modify: `docs/developer/native-tool-calling-protocol.md`
- Modify: `docs/developer/pointer-run-subagent.md`
- Modify: relevant design/agent docs referencing `general-worker`
- Test: agent registry and prompt assembly tests

**Interfaces:**
- General uses `run_subagent(agentId="self")` for isolated general work.
- Coder uses self for independent substantial slices and `explore` for broad read-only mapping.

- [ ] **Step 1: Add failing registry tests**

Assert `general-worker` is no longer registered and General no longer lists it
in `allowAgents`.

- [ ] **Step 2: Remove the agent and update references**

Remove runtime registration, tests, prompt text, and current documentation.
Retain backward-compatible display of historical trace ids as plain metadata;
do not require the removed registry entry to render old conversations.

- [ ] **Step 3: Update English prompt policy**

Add concise rules:

```text
When two or more substantial subtasks are independent and do not share mutable
state, emit multiple run_subagent calls with agentId "self" in the same turn.
Do not fork dependent tasks, overlapping writes, user-interactive work, or
desktop-control work.
```

Keep prompt file line lengths maintainable and do not mention prompt filenames.

- [ ] **Step 4: Update protocol documentation**

Document:

- same-turn multiple tool calls;
- self-fork leaf semantics;
- inherited tool metadata;
- concurrency limit;
- deterministic join;
- cancellation and errors.

- [ ] **Step 5: Run registry/prompt tests**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core agents:: run_subagent prompt --lib
```

Expected: tests pass.

---

### Task 8: Full verification

**Files:**
- Verify all changed Rust, prompt, and documentation files

- [ ] **Step 1: Format Rust**

```bash
cargo fmt --all -- --check
```

If it fails, run `cargo fmt --all`, then rerun the check.

- [ ] **Step 2: Run pointer-core tests**

```bash
CARGO_TARGET_DIR=target \
  cargo test -p pointer-core --lib
```

Expected: zero failures.

- [ ] **Step 3: Check both entry crates**

```bash
CARGO_TARGET_DIR=target \
  cargo check -p pointer-app -p pointer-server
```

Expected: both compile successfully.

- [ ] **Step 4: Run frontend checks**

```bash
npm run typecheck
```

If the repository uses a differently named script, run the existing equivalent
from `package.json`.

- [ ] **Step 5: Inspect lints and final diff**

Confirm:

- no new warnings attributable to changed files;
- no accidental package-lock changes from setup;
- no secret/config files;
- no unrelated changes;
- branch remains `feat/run-subagent-self-fork`.

