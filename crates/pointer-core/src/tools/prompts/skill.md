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

Progressive-disclosure tools for enabled Skills:

- **`skill_load_instructions`** — load the full **`SKILL.md`** body for an enabled Skill (layer 2).
- **`skill_read_resource`** — read one indexed resource file under that Skill (layer 3).
- **`skill_import`** — install a Skill from a `.zip` file or directory into the app skill store.

#### Usage

**`skill_import`**

- Use after downloading or cloning a Skill package to disk.
- `path` may be a `.zip` file, a single Skill directory (contains `SKILL.md`), or a parent directory of multiple Skill folders.
- Paths may be absolute or relative to the workspace root.
- Set `auto_enable` to `true` (default) so imported skills are available in the current conversation immediately.
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

**`read_resource`**

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

#### Methods

| Method | Purpose |
|--------|---------|
| **`load_instructions`** | Load the full **`SKILL.md`** body for an enabled Skill (layer 2). |
| **`read_resource`** | Read one indexed resource file under that Skill (layer 3). |
| **`import`** | Install a Skill from a `.zip` file or directory into the app skill store. |
