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

- An enabled skill's `name` / `description` in your instructions clearly matches
  the task → call **`skill_load_instructions`** first.
- The loaded skill body points at `references/`, `assets/`, or `scripts/` and
  the task needs that file → call **`skill_read_resource`**.
- The user provides a skill package on disk → call **`skill_import`**.

Never read or write skill files under the app data directory directly
(see **App data directory** in general rules). Always use
**`skill_load_instructions`** / **`skill_read_resource`** / **`skill_import`**.

#### Tools

- **`skill_load_instructions`** — load the full **`SKILL.md`** body for an enabled Skill (layer 2).
- **`skill_read_resource`** — read one indexed resource file under that Skill (layer 3).
- **`skill_import`** — install a Skill from a `.zip` file or directory into the app skill store.

#### Usage

**`skill_import`**

- Use after downloading or cloning a Skill package to disk.
- `path` may be a `.zip` file, a single Skill directory (contains `SKILL.md`), or a parent directory of multiple Skill folders.
- Paths may be absolute or relative to the workspace root.
- Set `auto_enable` to `true` (default) so imported skills are available immediately.
- May require user approval.

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

**`skill_load_instructions`**

- Call when an enabled skill's `name` / `description` clearly matches the task.
- Pass only enabled `skill_id` values.
- After loading, follow the skill body to complete the work.
- Do not reload the same skill unless its body is missing from context.

Example:

```json
{
  "function": {
    "name": "skill_load_instructions",
    "arguments": {
      "skill_id": "my-skill-id"
    }
  }
}
```

**`skill_read_resource`**

- Call only when the skill body points at `references/`, `assets/`, or `scripts/` and the task truly needs that file.
- `path` must be a resource-relative path, e.g. `references/api-guide.md`.
- This tool reads file content only; it does not execute scripts or binaries.

Example:

```json
{
  "function": {
    "name": "skill_read_resource",
    "arguments": {
      "skill_id": "my-skill-id",
      "path": "references/api-guide.md"
    }
  }
}
```
