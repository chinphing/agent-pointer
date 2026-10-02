# Rules: long-term constraints for the agent

English | [简体中文](../../zh-CN/user/rules.md)

"Rules" is not one thing in Pointer but **three**. They differ in where they are written, which part of the prompt they are injected into, and who they apply to — mix them up and you will be puzzled: "I clearly wrote a rule, so why did it not take effect?"

```
Written where                        Injected into                       Applies to
─────────────────────────────────────────────────────────────────────────────────────────
Settings → Agents → Personalization → [USER RULES] (system prompt)       → lead agent only
AGENTS.md in the workspace          → # Project Context (system prompt) → lead + sub-agents
A plugin's [rules] folder           → 【插件规则：…】(a user message)    → lead + sub-agents
```

## The three kinds of Rules side by side

| | User coding rules | `AGENTS.md` | Plugin `[rules]` |
| --- | --- | --- | --- |
| **Written where** | Settings → Agents → Personalization | An `AGENTS.md` file in the workspace | The rules folder inside the plugin package |
| **Injected as** | The `[USER RULES]` block in the system prompt | The `# Project Context` block in the system prompt | A user message appended after the history |
| **Applies to** | **lead agent only** | lead agent + sub-agents | lead agent + sub-agents |
| **Length cap** | 4000 characters (truncated beyond) | none | none |
| **Best for** | Global style and scope constraints | Project / directory-level conventions | Rules distributed together with a plugin |

## User coding rules

**Settings → Agents → Personalization**, a free-text block, **up to 4000 characters** (counted in characters); leave it empty to use only the product default rules.

It suits **long-lived constraints**, for example:

- Only change the behavior the user explicitly asked for; do not refactor along the way
- When something is ambiguous, state your assumptions first; do not widen the fix
- Put related but unrequested items into optional follow-ups instead of shipping them together

(These three are the examples built into the input box.)

What you write enters the system prompt under the heading `[USER RULES]`, **on every turn**.

> **Only the lead agent sees it.** The prompt of a sub-agent (a worker dispatched by `run_subagent`) does **not** contain `[USER RULES]`. To make sub-agents follow the same constraints, write them into `AGENTS.md` (see below).

Past 4000 characters it does **not** raise an error; it truncates and appends `…(truncated at 4000 chars)`. So "I wrote it but it had no effect" is sometimes really "it got cut off".

## AGENTS.md

`AGENTS.md` is a **file** that lives in the workspace and travels with the repository — the right place for "how work gets done in this project".

### Which ones get loaded

Only two kinds:

| Location | Scope |
| --- | --- |
| `~/.pointer/AGENTS.md` | Global; applies to all your workspaces |
| The `AGENTS.md` files from the **git root** down through each level to the **workspace** | That directory and everything below it |

```
~/…/.pointer/AGENTS.md                     ← global, always loaded first
repo/AGENTS.md                             ← git root
repo/packages/AGENTS.md                    ← intermediate levels
repo/packages/web/AGENTS.md                ← the workspace (the folder this chat is bound to)
```

With the workspace at `repo/packages/web` in the example above, all four are loaded; the sibling `repo/packages/api/AGENTS.md` is **not** loaded — side branches are not scanned.

### How the chain is computed

1. From the workspace, walk **up** to find `.git` and get the git root (at most 64 levels up)
2. Walk **down** from the git root to the workspace, **counting both ends**, taking at most one file per level
3. When no git root is found, only the one in the workspace root directory is read
4. When the workspace is the filesystem root (`/`, `C:\`), no project level is read

Each file carries its own label in the prompt — its path relative to the git root: `AGENTS.md`, `packages/AGENTS.md`, `packages/web/AGENTS.md`.

### Notes

- The file name **must be `AGENTS.md`**, case-sensitive, with no other naming variants
- There is **no** `.agents/` directory convention and no `.agents/AGENTS.md`; putting it under `.agents/` has no effect
- `AGENTS.md` goes into both the lead agent's and the sub-agents' prompts, so this is where rules that "sub-agents must follow too" belong

## Plugin `[rules]`

A plugin can declare a rules folder in its own manifest:

```toml
[rules]
path = "rules/"
```

Once the plugin is enabled, the `*.md` / `*.mdc` files in that folder are joined into one **user message** appended after the conversation history on every turn, starting with:

```
【插件规则：{插件名}（{插件 ID}）】
```

Because it is a user message rather than a system prompt, it sits after the history (the closer to the end of the request, the stronger the model's attention).

Plugin rules **cannot be edited on their own**: they ship with the plugin, and only editing or disabling the plugin changes them.

## Priority and stacking

The three kinds of Rules **stack**; the code does **no overriding and no de-duplication** — they are all joined into the same prompt, and avoiding conflicts is up to you.

The concatenation order in the system prompt (lead agent turn):

```
Role prompt → UI language rules → tool descriptions → [Environment]
  → memory / user profile → [USER RULES] → # Project Context
```

The priority stack the product gives the agent (written into the built-in communication rules) is:

```
This conversation → [SESSION SCOPE] → [USER RULES] → Project Context
  → Agent profile → optional follow-ups
```

Two things to note:

- Plugin `[rules]` are **not** in this stack — they appear as a user message, after the history
- Later content is "closer" in the prompt, but the stack itself describes **semantic priority**: `[USER RULES]` outranks `# Project Context`

Practical advice when writing rules:

| You want | Write it in |
| --- | --- |
| Global style / scope constraints | User coding rules |
| Conventions for one project (sub-agents included) | That project's `AGENTS.md` |
| Rules that travel with a plugin | The plugin's `[rules]` |
| One-off requirements for this task only | Say them in the conversation, or use `[SESSION SCOPE]` |

## FAQ

**You wrote user rules but sub-agents ignore them**

That is by design: `[USER RULES]` is **only injected into the lead agent's turn**. Rules that sub-agents must follow too belong in `AGENTS.md`.

**A rule "was written but has no effect"**

Check in order:

1. Whether the input box is empty (empty = only the product default rules)
2. Whether it passed 4000 characters and got truncated — the cut-off part never enters the prompt
3. Whether you expected sub-agents to follow it (see above)
4. If it is `AGENTS.md`: whether the file is on the workspace path, or whether you put it under `.agents/` (that does not work)

**`AGENTS.md` under `.agents/AGENTS.md` does nothing**

Pointer does not read the `.agents/` directory. It must be an `AGENTS.md` in the workspace (or at some level between the git root and the workspace).

**Two rules contradict each other**

The code does no conflict resolution; both go into the prompt. Rewrite them so they do not conflict, or put the project-level exception into `AGENTS.md` (it is "closer" to the current project than user rules).

**Do I need to restart after changing `AGENTS.md`?**

No. The files in the workspace are re-read on every turn.

## Related

- [Settings overview](settings.md) — "Agents → Personalization" and the neighbouring settings
- [workspace.md](workspace.md) — which folder the workspace is and how to switch it
- [Sub-agent settings](subagents.md) — sub-agent tiers and delegation
