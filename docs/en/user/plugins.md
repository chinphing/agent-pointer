# Pointer plugin development guide (user level)

English | [简体中文](../../zh-CN/user/plugins.md)

> For plugin authors. This document explains **how to write a Pointer native plugin from scratch** (`pointer-plugin.toml`),
> and how to import an existing Codex / Claude Code plugin as the Pointer native format.
> The plugin machinery lives in `crates/pointer-core/src/plugins/` (manifest / importer / activation / registry).

---

## 1. What a plugin is

A Pointer plugin is a **directory** containing:

- `pointer-plugin.toml` — the single runtime manifest (**native format, must exist**)
- several **capability unit** subdirectories: `skills/`, `agents/`, `rules/`, `tools` (inline declarations), `hooks/`, `mcp_servers` (inline declarations)

After the plugin is enabled, its capability units are wired into Pointer's registries automatically:

| Capability unit | Description | Status |
|---|---|---|
| `skills/` | SKILL.md skills (same format as user skills) | ✅ registered on enable, added automatically to the general assistant's enabled list |
| `agents/` | AGENT.md sub-agents (workers) | ✅ registered on enable |
| `rules/` | Rule files (`.md` / `.mdc`), injected every turn | ✅ injected on enable |
| `[[tools.tool]]` | Out-of-process tools (sidecar executables) | ✅ registered on enable (**must carry an `exec` execution carrier**) |
| `hooks/` | hooks.json (Claude semantics) | ✅ registered on enable (PreToolUse blocking / PostToolUse observation; a failing script is allowed through by default, an entry can set `fail_closed` to switch to blocking). **SessionStart / SessionEnd** are also supported (Run level: fired at the start / end of every Run, pure observation with no blocking; note that this differs from Claude's "whole session" semantics — see the note below) |
| `mcp_servers` | MCP server declarations | ✅ started on enable (stdio or http, tools registered automatically; a crash restarts automatically, and once retries hit the cap the plugin status shows "running abnormally"). **Global MCP** (not plugin-scoped) is also supported: the "MCP" section of the settings panel is **configured directly in the UI** (persisted to local user configuration, supports remote URLs / local commands), with tools named `mcp.<server>.<tool>`; see [`mcp.md`](mcp.md) |

> After import a plugin is a one-off **snapshot**: later changes in the source directory are not synced automatically, and a re-import is required.

> **SessionStart / SessionEnd semantics**: in Pointer these two events map to the **Run level** — fired once at the start / end of every Run, rather than Claude Code's "whole session" (open the terminal once / exit once). A plugin written for Claude semantics will, after migration, have SessionStart **fire once per Run**. SessionEnd fires once for each of the Run's finished / failed / cancelled terminal states. Both are pure observers (they cannot block; a failing script is only logged).

---

## 2. Directory structure

```text
my-plugin/
├── pointer-plugin.toml        # required: plugin manifest
├── skills/                    # optional: skills (each subdirectory holds a SKILL.md)
│   └── hello/
│       └── SKILL.md
├── agents/                    # optional: sub-agents (each subdirectory holds an AGENT.md)
│   └── worker/
│       └── AGENT.md
├── rules/                     # optional: rules (.md / .mdc, collected recursively)
│   └── guardrail.md
├── hooks/                     # optional: hooks.json (PreToolUse / PostToolUse / SessionStart / SessionEnd)
│   └── hooks.json
├── bin/                       # convention: put sidecar executables here
│   └── demo-tool
└── .mcp.json                  # optional: MCP declarations (Claude convention, P2)
```

A plugin id is a **reverse domain name** (`com.example.demo`), installed into `~/.pointer/plugins/<id>/`.

---

## 3. pointer-plugin.toml reference

```toml
# ── required sections ───────────────────────────────────
[plugin]
id = "com.example.demo"        # reverse domain, globally unique, lowercase letters/digits/hyphens/dots only
name = "Demo plugin"           # required
version = "1.0.0"              # required
api_version = "v1"             # optional, default v1 (currently only v1 is supported)
description = "Does something" # optional
author = "Author"              # optional
license = "MIT"                # optional

# ── permission declarations (optional; P1 uses them as an approval fallback) ──
[permissions]
network = []                   # network domain allowlist (enforced by the P2 process wrapper layer)
filesystem = []                # filesystem path scope
env = []                       # process environment variable allowlist
secrets = []                   # declarative secret references ${secrets.X}

# ── capability unit directories (optional) ──────────────
[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"

[hooks]
path = "hooks/"

# ── MCP server declarations (optional; started in P2, currently parsed and validated) ──
[mcp_servers]
[[mcp_servers.server]]
name = "demo"
transport = "stdio"            # default stdio
command = "bin/demo-mcp"
args = ["serve"]
env = { KEY = "value" }

# ── out-of-process tools (optional) ─────────────────────
[tools]
[[tools.tool]]
name = "demo_hello"            # tool name (registered into the ToolRegistry)
risk_level = "low"             # low | medium | high, default low
requires_approval = false
parallel_eligible = false
description = "Greeting demo tool"
# parameter JSON Schema (optional; when absent only the description text is given)
schema = { type = "object", properties = { name = { type = "string" } } }

# execution carrier: sidecar or mcp, pick one (required; without exec the tool is rejected)
[[tools.tool.exec]]
command = "bin/demo-tool"      # relative to the plugin root
transport = "sidecar"          # sidecar (P1) | mcp (P2, must name a server)
timeout_ms = 30000             # optional
env = { KEY = "value" }        # optional

# ── fields the import converter cannot map are kept here (marked "unmapped" in the import report) ──
[metadata]
original = "…"
```

### Validation rules (non-conforming plugins are marked `rejected` and cannot be enabled)

- `[plugin].id` must be a reverse domain name and non-empty;
- `name` / `version` non-empty; `api_version` supports only `v1`;
- `[[tools.tool]]` **must have `exec`** (sidecar or mcp server, pick one);
- `exec.transport` is only `sidecar` | `mcp`; with `mcp`, `exec.server` must be given (referencing `[[mcp_servers.server]].name`);
- `risk_level` is only `low` | `medium` | `high`.

---

## 4. Writing a plugin from scratch (example)

```bash
# 1. create the directories
mkdir -p ~/dev/my-plugin/{skills/hello,agents/worker,rules,bin}

# 2. write the manifest (see the section above; use a reverse domain for id)
cat > ~/dev/my-plugin/pointer-plugin.toml <<'EOF'
[plugin]
id = "com.example.demo"
name = "Demo plugin"
version = "1.0.0"
description = "Demo capability units"

[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"
EOF

# 3. write a skill (a standard SKILL.md with name/description frontmatter)
cat > ~/dev/my-plugin/skills/hello/SKILL.md <<'EOF'
---
name: hello
description: Greeting demo skill
---
Invoke this skill when the task matches…
EOF

# 4. write a sub-agent (a standard AGENT.md)
cat > ~/dev/my-plugin/agents/worker/AGENT.md <<'EOF'
---
id: worker
name: Demo Worker
description: Demo sub-agent
role: worker
enabled: true
---
You are a demo worker agent.
EOF

# 5. write a rule
cat > ~/dev/my-plugin/rules/guardrail.md <<'EOF'
# Guardrails
Always reply in Chinese.
EOF

# 6. package (optional; both a zip and a directory can be imported)
cd ~/dev && zip -r my-plugin.zip my-plugin
```

**How to import** (pick one of three):

- Desktop settings → Plugins → "Import directory" or "Import zip";
- Web settings → Plugins → "Import" with a directory/zip path, or "Upload zip";
- API: `POST /api/plugins` (path) or `POST /api/plugins/import-zip` (zip bytes).

After importing, go to the plugins page and "Enable". Enabling automatically:

- registers skills/agents/rules/tools into their registries;
- adds the plugin's skills to the **general assistant**'s enabled list (the skills panel shows a "Plugin · <plugin name>" badge);
- disabling a plugin → capability units are unregistered (the enabled list is kept and restored on re-enable);
- uninstalling a plugin → capability units are unregistered + removed from the enabled list.

---

## 5. How a sidecar tool returns results

A `sidecar` tool is an executable inside the plugin. When Pointer calls it:

- `exec.command` is resolved relative to the plugin root (or as an absolute path);
- output convention: stdout is returned to the model as **JSON**, of the form:

```bash
#!/bin/sh
# read the tool arguments (JSON) from stdin, process them, then print JSON
echo '{"ok": true, "result": "hello"}'
```

(Example: the `bin/demo-tool` used in the tests of `crates/pointer-core/src/plugins/activation.rs`.)

> In P1 the `ToolHandler` is a synchronous closure; a sidecar runs as a separate process and its timeout is controlled by `timeout_ms`.

---

## 6. Importing from Codex / Claude plugins

Pointer does not load the Codex / Claude format at runtime; it performs a **one-off conversion** to the native format:

- **Claude plugin**: `.claude-plugin/plugin.json` + `skills/` `agents/` `commands/` `hooks/` `.mcp.json`
- **Codex plugin**: `.codex-plugin/plugin.json` (or a root `plugin.json`) + `skills/` (some under `.agents/` etc.)

On import it detects by the **Pointer → Codex → Claude** priority and prints a report:

```text
"superpowers" (local.superpowers): skills, hooks, pointer-plugin.toml; skipped agents, commands; 1 unmapped
```

- `converted`: capability units that were converted (e.g. `skills`, `commands→skills`, `mcp_servers`);
- `skipped`: parts that do not exist in the source or are unsupported (e.g. `agents`, `commands`);
- `unmapped`: manifest fields that could not be mapped (kept under `[metadata]`, so no information is lost).

> In Pointer, commands are folded into skills (generating `skills/<name>/SKILL.md`); hooks run once the plugin is enabled
> (PreToolUse can block, PostToolUse observes; with `"fail_closed": true` a failing / timed-out script blocks the tool call); MCP declarations are parsed but not started yet. After importing, verify the actual capabilities with "enable + check the skills panel".

---

## 7. Plugin directory location and lifecycle

| Item | Description |
|---|---|
| User-level plugins | `~/.pointer/plugins/<id>/` |
| Authorisation state | `~/.pointer/plugins/.auth.json` (enabled / enabled_at / fingerprint) |
| Source labelling | Skills panel badge: `Plugin · <plugin name>`; removed from the enabled list on uninstall |
| Idempotent | Re-importing the same id deletes the old directory first and then writes; re-enabling is safe |
| Snapshot semantics | Import is a one-off copy; later changes in the source directory are not synced automatically |

### Plugin state machine

```text
discovered → needs_reauth (hash changed) → enabled
        └── rejected (validation failed, cannot be enabled)
enabled → disabled (disabled, capability units unregistered) → enabled (re-enabled)
enabled → uninstall (uninstall: delete directory + clear authorisation + remove skills)
```

---

## 8. FAQ

**Q: Why are plugin skills enabled automatically?**
When a plugin is enabled, its skills are added automatically to the general assistant's enabled list (persisted). Every refresh of the skills list / app start also reconciles them as a fallback.
Disabling a plugin does not clear the enabled list, so re-enabling restores it; uninstalling does clear it.

**Q: Why was my plugin marked rejected?**
Most often: `[plugin].id` is not a reverse domain name, a tool lacks `exec`, `exec.transport` is neither `sidecar`/`mcp`, or `risk_level` is invalid.
The import report gives the specific reason.

**Q: Why did my skills directory not take effect?**
Check that every skill under the directory pointed to by `[skills] path = "skills/"` is a **separate subdirectory** containing `SKILL.md`;
the skill frontmatter needs `name` and `description`.

**Q: When can hooks / MCP run?**
The hooks executor is implemented (PreToolUse / PostToolUse / SessionStart / SessionEnd, sidecar process + JSON decision); MCP integration (P2) is complete.

**Q: I updated the plugin source directory but Pointer still shows the old version?**
A plugin is a snapshot. Re-import it (the same id overwrites) or update `~/.pointer/plugins/<id>/` manually and then restart / re-enable.
