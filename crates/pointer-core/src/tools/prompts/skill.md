---
schema:
  type: object
  properties:
    skill_id:
      type: string
    path:
      type: string
  additionalProperties: true
---

### `skill`

Progressive-disclosure tools for enabled Skills.

Available only when the active agent is **general** and these tools are granted.
Other agent profiles must not load or invoke skills.
Do not invent skill contents from memory — use the tools below.

#### When to use

**Priority for attachment / capability gaps:** (1) enabled skill from the
**可用 Skills** / `<available_skills>` index → (2) **`find-skills`** / install → (3) ad-hoc code last.
Never run `npx skills find` when an enabled skill already matches.

Each enabled skill in `<available_skills>` includes **`<name>`** (use as **`skill_id`**
in **`skill_read`**) and **`<location>`** (path to `SKILL.md`). The skill directory
is the parent of `<location>`. Resolve `scripts/`, `references/`, `{baseDir}`, and
other relative paths against that directory; pass **absolute paths** to **`terminal`**.

- An enabled skill's `<description>` clearly matches the task → **`skill_read`**
  with **`skill_id`** = `<name>` (loads `SKILL.md`).
- The loaded skill body points at `references/`, `assets/`, or `scripts/` and
  the task needs that file → call **`skill_read`** with **`skill_id`** and **`path`**.
- The user provides a skill package on disk → call **`skill_import`**.

Never read or write skill files under the app data directory directly
(see **App data directory** in general rules). Always use
**`skill_read`** / **`skill_import`**.

**Updating user skills:** do **not** edit `~/.pointer/skills/` yourself.
Delegate with **`run_subagent(agentId="coder")`** — when-to / **`workspaceRoot`** /
**`goal`** / **`context`**: follow the **`run_subagent`** tool doc (**`coder`**).
Skill root is typically `~/.pointer/skills/{skill-name}/` (or `~/.pointer/skills/`
when creating). The **coder** worker uses **`file_*`** + **`skill-creator`**
(prefer small **`file_edit`** patches; avoid whole-file overwrites of **`SKILL.md`**).
**Install only:** user-supplied zip or directory → **`skill_import`** (general lead).

#### Tools

- **`skill_read`** — read an enabled Skill: omit **`path`** for **`SKILL.md`** instructions (layer 2); pass **`path`** for a bundled resource file (layer 3).
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
- **Instructions:** omit `path`, or pass `"SKILL.md"`.
- **Resource file:** pass `path` relative to the skill directory (parent of `<location>`), e.g. `references/api-guide.md`.
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
      "skill_id": "my-skill-id"
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
