# Sub-agent settings

English | [简体中文](../../user/subagents.md)

Pointer can delegate subtasks to specialised workers via **run_subagent** (e.g. **explore** to explore the codebase, **coder** to write code).

## User settings

Adjustable in **Settings → System settings → Execution** (or `settings.json`):

| Key | UI | Description |
|----|------|------|
| `maxToolRounds` | This turn | Tool-loop cap for the main session's current turn (default **5000**) |
| `maxSubAgentToolRounds` | Subtasks | Tool-loop cap inside each subtask, independent of the main session (default **500**, maximum 500) |
| `maxParallelSubAgents` | Tool parallelism → Sub-agents | Cap on sub-agents running concurrently in this session, **shared by foreground and background**; background terminals consume this quota too. Foreground terminals count against "General tools" |
| `maxParallelToolCalls` | Tool parallelism → General tools | Cap on read tools (including foreground terminals) executing concurrently in the same turn |
| `maxSubAgentSpawnDepth` | (no UI yet) | Maximum nesting depth for delegation (default **2**: main agent + one level of sub-delegation). **Every spawn consumes one level** (`self` forks included); an agent at the cap can no longer delegate |
| `maxChildrenPerAgent` | (no UI yet) | Cap on sub-agents alive at the same time for a single agent (default **8**, range **1–32**). A delegation past the limit returns an error immediately; it is not queued |

## Built-in behaviour

- The built-in **coder** already allows the **explore** sub-agent by default, for codebase exploration
- When the assistant delegates **explore** / parallel tasks of its own, they run in the background by default: the main session can keep talking while the sidebar keeps spinning for that conversation. Clicking **Stop** cancels these background tasks; **Send now** or **Stop waiting** on the tool row only ends the current reply (including the wait), and the background keeps running. A single background task can be ended with **End task**. When a background task finishes and the main session was not replying at that moment, it continues in the **same conversation** with one more turn that summarises the result for you (the UI first shows "Background task finished: <task name>"). If the result still needs changes, the assistant carries on with that same finished subtask without losing the earlier process. When the result is needed right away in the current turn, the assistant can still choose to wait in the foreground. Background terminals started inside a sub-agent also appear in the background task list above the input box.
- When **general** delegates to **computer**, **every** subtask needs user confirmation; a task or turn approved earlier **cannot** be carried over automatically
- **Nesting**: sub-agents can delegate further themselves (two levels by default at most: main agent + sub-agent + grandchild agent); the execution process is shown nested by level inside the subtask box; a `self` fork is labelled `coder (fork)` and the like, so it is easy to tell apart from a real worker. The number of sub-agents alive at the same time per agent defaults to a cap of 8; delegations beyond that return an error immediately
- **Deep questions**: `ask_user` inside a sub-agent (including deeply nested ones) appears on the fixed bar at the top of the conversation area, so you do not have to expand the subtask box first; after you answer, the bar briefly shows "Selected X" and then disappears

## Custom agents

If you write a custom **Lead Agent** (`AGENT.md`) in a workspace or extension, you can control the list of delegatable workers via the frontmatter **`allowAgents`**. For the format, parameters and explore conventions see **[`../../developer/pointer-run-subagent.md`](../../developer/pointer-run-subagent.md)**.
