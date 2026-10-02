# run_subagent and allowAgents

English | [简体中文](../../zh-CN/developer/pointer-run-subagent.md)

> The **`maxSubAgentToolRounds`** / **`maxSubAgentSpawnDepth`** settings are covered in **[`../user/subagents.md`](../user/subagents.md)**.

For the design comparison with Cursor Explore, the Lead→explore conventions and more, see **[Design document: the explore sub-agent](../../zh-CN/design/explore-subagent-for-coder.md)**.

## Configuration

### Lead Agent (`AGENT.md` frontmatter)

| Key | Type | Notes |
|----|------|------|
| `allowAgents` | `string[]` | The list of **worker** `agentId`s this Lead may pass to **`run_subagent`**; sorted and de-duplicated on load. Only the metadata of these ids is injected into the system prompt (see `delegatable_sub_agents_system_block`). Built-in matrix: **`general`** → `coder` / `computer` / `explore`; **`coder`** → `explore`; **`explore`** → `explore` (read-only, no `coder`); **`computer`** has no `allowAgents` (always a leaf, always foreground). |

**`agentId="self"`** is not configured in `allowAgents`: any agent that has the **`run_subagent`** tool can fork itself, to execute with an isolated context (general, coder, and so on). A fork inherits the parent's `allowAgents` and, **while it still has depth budget, may delegate again** (at `maxSubAgentSpawnDepth` it is a leaf); every spawn (fork included) really consumes one level of depth. See the **`run_subagent`** tool documentation and the lead's **`AGENT.md`** for details.

### `agentId` set to your own id = `self`

When the model writes `agentId: "coder"` inside coder, or `agentId: "general"` inside general, the meaning is "one more of me", which is exactly a fork; but its own id is normally not in `allowAgents`, so the registered path is bound to fail. The host therefore rewrites it to `self` **during the tool-batch preparation stage** (`agent_tool_pass`, earlier than wave planning), so that orchestration, execution and the UI all see the same target id, and records `run_subagent: own agent id resolved as self fork`.

There is only one case where it is not rewritten:

| Case | Behavior | Reason |
|------|------|------|
| That id is explicitly written into `allowAgents` | Keep the registered path | A fork (inheriting the current snapshot) and a new instance have different semantics, and explicit configuration wins |

`workspaceRoot` can be used for a self fork, and takes precedence over the parent conversation's workspace. At runtime it is validated with the same rules as for all sub-agents: it must be an existing absolute directory and is canonicalized; an invalid path produces an explicit error and does not silently fall back to the parent workspace.

Example (`crates/pointer-core/src/agents/coder/AGENT.md`):

```yaml
allowAgents:
  - explore
```

### User settings (`settings.json` / frontend settings API)

| Key | Type | Notes |
|----|------|------|
| `maxSubAgentToolRounds` | `number` | The round cap for the tool loop **inside each** `run_sub_agent`, independent of the main conversation's `maxToolRounds` (default **500**, maximum 500). |
| `maxSubAgentSpawnDepth` | `number` | Maximum nesting depth of `run_subagent` (default **2**: the main agent + one level of delegation). **Every spawn (`self` forks included) really consumes one level**; an agent that reaches the cap is a leaf with no `run_subagent`. |
| `maxChildrenPerAgent` | `number` | Maximum number of simultaneously live child Agents per agent instance (default **8**, clamped **1–32**). A spawn beyond the limit returns a deterministic `ERROR` (stating the limit) instead of silently queueing. |

## `run_subagent` parameters

| Field | Required | Notes |
|------|------|------|
| `agentId` | ✓ | A worker id (must be in the lead's `allowAgents`), or the reserved value **`self`** (fork the current agent; no `allowAgents` needed) |
| `goal` | ✓ | Subtask goal + completion criteria |
| `context` | ✗ | Verified facts, paths, dependency summaries, etc. (including the `localPath` / media ref of list files) |
| `background` | ✗ | **`self`** / **`explore`** / **`coder`**: **omitted / `true` = background** (returns `jobId` immediately; to get the result this turn use **`job.await`**, or wait for the idle push). **`false` = foreground join**. **`computer`** is always foreground |

The host writes the task into the sub-agent's **system** (**Assigned task**); the first user message is a short stub and does not repeat the goal. The list-file path goes in **`context`** and is filled into **`work_items_source`** by the worker planner at **`task_board_init`** time.

## `general` delegating to `coder` / `computer`

**`coder` can be delegated to directly; `computer` requires `ask_user` consent before every delegation** (for the current task; a previous consent does not carry over). See the **Delegation** section of **`general/AGENT.md`** and the **`ask_user`** / **`run_subagent`** tool documentation for the policy.

| Worker | Consent | Notes |
|--------|------|------|
| **`coder`** | Not needed | All repository source work (analysis, edits, tests); Skill writes. The next tool call = **`run_subagent(coder)`**, and the parent thread does not do reconnaissance itself; user facts go into **`context`** and are handled by **coder** (with **`explore`** when needed). |
| **`computer`** | Needed (**`ask_user`**) | Local browser / desktop operations. Ask for consent with **`ask_user`** for **each** subtask (asking only in the body text is not allowed); it may be skipped only when **the current user message** already authorises this desktop operation. |

**Stays on the general main thread**: conversation, general knowledge, **`skill_*`**, attachments; simple Q&A that needs no repository reading.

## `general` → `coder` working directory

general has no Composer workspace picker. Before delegating to **coder**, ask the user in the conversation for the **absolute path of the project**:

| User reply | `run_subagent` parameter | Host behavior |
|----------|---------------------|----------|
| Gives a path | **`workspaceRoot`** = that absolute path (**required**) | Validates that the directory exists and uses it as coder's workspace |
| No project path given | **`workspaceRoot`** = the current session workspace (**required**, cannot be omitted) | Uses the directory already selected in the Composer, or the default sandbox (see [workspace-root.md](../../zh-CN/developer/workspace-root.md)) |

At runtime this is synced to the frontend's `conversation.workspaceRoot` via the **`workspace_updated`** stream event (a temporary directory triggers a toast). When the sub-agent finishes it is emitted again to **restore the parent agent's workspace** (the backend thread-local is restored by `AgentWorkspaceGuard`). Implementation: `workspace_delegation.rs`, `run_subagent_delegation.rs`.

## Behavior summary

- **One spawn or split**: one result, one call; split only when the child loop's round cap would be (or has been) hit, accept A before starting B, and do not retry the original bundle. See the tool documentation **One spawn vs split**.
- **`run_subagent` return value**: the parent model sees the **`content`** in the tool result (the last assistant's Markdown handoff). If that sub-Agent still has unfinished background jobs, the same JSON carries **`openBackgroundJobs`** (`jobId`, `status`, `kind`, `title`, no body), and the parent model uses those ids for `job.status` / `job.await`. The process rows are in the database but outside the lead's context; the design for retrieving them by child thread is in [`../design/session-search-scope-extension.md`](../../zh-CN/design/session-search-scope-extension.md) (not implemented). `agentId` / `agentName` are metadata. **Do not** write the child loop's `reasoning` into this JSON: thinking is attached only to the sub-Agent's own assistant turn, to be sent back verbatim to the next API call. If a historical conversation already has a `reasoning` field written into it, that was the old behavior.
- **`self` fork**: forks the current agent's execution snapshot (profile, tools, skills, workspace, `allowAgents`); an independent `local_history` and trace. **While it still has depth budget** it keeps `run_subagent` / `job` (and may delegate again); at `maxSubAgentSpawnDepth` it is a leaf; every spawn (fork included) really consumes one level of depth. The **`ask_user`** in the parent's allowlist is inherited by the fork (`inherit_to_subagent` allows it by default), so a subtask can ask for clarification directly without going back to the main conversation.
- **Parallel wave**: several independent **`self`** and/or **`explore`** calls in the same assistant turn can share an owned-outcome parallel wave (bounded by `maxParallelSubAgents`); **`coder`** / **`computer`** stay serial (`coder` may run in the background, but does not enter the parallel wave). Foreground waves, serial delegation and background jobs **share** the per-conversation worker pool (what is split is whether to wait, not two separate gates). Dependent tasks, overlapping writes, and tasks that need user interaction or desktop control must not run in parallel.
- **Background**: for **`self`** / **`explore`** / **`coder`**, `run_subagent` with **`background` omitted is background** (the same as `true`) and returns **`{ jobId, status: "running", kind: "subagent" }`** immediately. This tool result **stays a handle forever**: even after the subtask finishes it does not write the worker's final report back (aligned with Cursor's background Task / Codex `spawn_agent`). The final report reaches the **direct initiator** only through **`job.await`** or (for jobs the lead itself opened) the idle merged push after the parent turn ends. Background jobs a sub-Agent opens itself are not pushed to the lead and do not start another turn automatically; that sub-Agent uses `job.await`. **`job.list` / `job.status` carry only metadata and `claimed`, with no `content`**. Only an explicit **`background: false`** joins in the foreground. **`computer`** always joins. When several `coder` calls in the same turn write the same batch of files, use **`background: false`** or `job.await` before starting the next write. `job.await` with `mode=any` wakes only on an **unclaimed terminal state**: the terminal state enters `jobs[]` and is claimed. Tool activity inside a sub-agent does not wake the parent model. `mode=all` waits for all of them
- **Delegating to `computer`**: as before sending from the Computer lead, it blocks waiting for the macOS permission wizard (desktop) and the screen choice (`computer_monitor_pick_required` → `Composer.beginSubagentMonitorPickFlow`); a single screen is selected automatically, multiple screens show a picker, and an already-chosen screen is reused.
- Nested delegation: depth is controlled by **`maxSubAgentSpawnDepth`** (default 2) and fan-out by **`maxChildrenPerAgent`** (default 8, clamped 1–32). **Every spawn (`self` forks included) consumes one real level of depth**; a sub-agent at the maximum depth is a leaf with no `run_subagent` tool. `computer` is always a leaf and always foreground. A grandchild agent's `content` goes back only to its direct parent level; the lead only gets the parent level's handoff and the `openBackgroundJobs` metadata.

## Built-in worker `explore`

- **Purpose**: read-only codebase reconnaissance (`file`'s list / glob / grep / read), producing a structured summary for the main conversation's **coder** to continue Plan / Implement; it does not write files, run a shell, or run `read_lints`.
- **Boundary vs. main-thread reconnaissance**: in step 2 of its **Routine workflow**, coder uses `file` to gather evidence only until it can edit code / run tests; **explore** is for "many `file` rounds would blow up the main conversation" or for audit-style reconnaissance where the completion shape must be fixed in the **`goal`** (e.g. a bidirectional trace, coverage). Details are in **Delegating to the `explore` worker** in the coder prompt (`crates/pointer-core/src/agents/coder/AGENT.md`).
- **Enabling**: put **`explore`** into **`allowAgents`** in the Lead Agent's **`AGENT.md`**; if it is not listed, the **`run_subagent`** target validation fails. The built-in **coder** is configured with it by default.
- **Prompt**: the strategy is in `crates/pointer-core/src/agents/explore/AGENT.md`; the main Agent writes the goal, scope, completion criteria and verified facts with **`goal`** + **`context`**.
