---
schema:
  type: object
  properties:
    skill_id:
      type: string
    path:
      type: string
  required:
    - skill_id
    - path
  additionalProperties: true
---

### `skill`

Progressive-disclosure tools for enabled Skills.

Available when these tools are granted on the active agent
(e.g. **general**, **coder**).
Do not invent skill contents from memory — use the tools below.

**`skill_read` loads a skill** via **`skill_id`** and required **`path`**.
Use it only for enabled Skills — not for arbitrary workspace or project files.

#### When to use

**Priority for attachment / capability gaps:** (1) enabled skill from the
**可用 Skills** / `<available_skills>` index → (2) **`find-skills`** / install → (3) ad-hoc code last.
Never run `npx skills find` when an enabled skill already matches.

Each enabled skill in `<available_skills>` includes **`<name>`** (use as **`skill_id`**
in **`skill_read`**) and **`<location>`** (path to `SKILL.md`). The skill directory
is the parent of `<location>`. Resolve `scripts/`, `references/`, `{baseDir}`, and
other relative paths against that directory; pass **absolute paths** to **`terminal`**.

- An enabled skill's `<description>` clearly matches the task → **`skill_read`**
  with **`skill_id`** = `<name>` and **`path`** = `SKILL.md` (loads instructions).
- The loaded skill body points at `references/`, `assets/`, or `scripts/` and
  the task needs that file → call **`skill_read`** again with the same **`skill_id`**
  and **`path`** set to that skill-relative file.
- The user provides a skill package on disk → call **`skill_import`**.

Never read or write skill files under the app data directory directly
(see **App data directory** in general rules). Always use
**`skill_read`** / **`skill_import`**.

**Updating user skills:** do **not** edit `~/.pointer/skills/` yourself.
Delegate with **`run_subagent(agentId="coder")`** — when-to / **`workspaceRoot`** /
**`goal`** / **`context`**: follow the **`run_subagent`** tool doc (**`coder`**).
Skill root is typically `~/.pointer/skills/{skill-name}/` (or `~/.pointer/skills/`
when creating). The **coder** worker applies edits (prefer small patches;
avoid whole-file overwrites of **`SKILL.md`**) and may use **`skill-creator`**.
**Install only:** user-supplied zip or directory → **`skill_import`** (general lead).

#### Tools

- **`skill_read`** — read an enabled Skill. **`path`** is required:
  `SKILL.md` for instructions (layer 2); a bundled resource path for layer 3.
- **`skill_import`** — install a Skill from a `.zip` file or directory into the user library (`~/.pointer/skills/`). Does **not** replace **`run_subagent` → coder** for edits.

#### Usage

**`skill_import`**

- Use after downloading or cloning a Skill package to disk.
- `path` may be a `.zip` file, a single Skill directory (contains `SKILL.md`), or a parent directory of multiple Skill folders.
- Paths may be absolute or relative to the workspace root.
- Set `auto_enable` to `true` (default) so imported skills are available immediately.
- May require user approval.
- Re-importing an existing skill id replaces the whole skill directory.

Example:

```json
{
  "function": {
    "name": "skill_import",
    "arguments": {
      "path": "downloads/my-skill.zip",
      "auto_enable": true
    }
  }
}
```

**`skill_read`**

- Call when an enabled skill's `name` / `description` clearly matches the task.
- Pass only enabled `skill_id` values.
- Always pass **`path`** (required). Never omit it.
- **Instructions:** `path` = `SKILL.md` (re-read from disk each call; edits apply immediately).
- **Resource file:** `path` relative to the skill directory (parent of `<location>`),
  e.g. `references/api-guide.md`.
- **Scripts:** after **`skill_read`**, run bundled scripts via **`terminal`** using absolute paths (`{baseDir}` in the skill body is expanded on load; otherwise use `dirname(<location>)` + relative path).
- After loading instructions, follow the skill body to complete the work.
- Do not reload the same skill unless its body is missing from context.
- Resource reads return file content only; they do not execute scripts or binaries.

Example — load instructions:

```json
{
  "function": {
    "name": "skill_read",
    "arguments": {
      "skill_id": "my-skill-id",
      "path": "SKILL.md"
    }
  }
}
```

Example — read a reference file:

```json
{
  "function": {
    "name": "skill_read",
    "arguments": {
      "skill_id": "my-skill-id",
      "path": "references/api-guide.md"
    }
  }
}
```
