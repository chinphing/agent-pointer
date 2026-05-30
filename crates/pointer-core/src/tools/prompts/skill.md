---
schema:
  type: object
  properties:
    method:
      type: string
      enum:
        - load_instructions
        - read_resource
    skill_id:
      type: string
    path:
      type: string
  additionalProperties: true
---

### `skill`

Progressive-disclosure tools for enabled Skills. Call the **`skill`** tool with **`method`**:
**`load_instructions`**, **`read_resource`**.

#### Usage

**`skill:load_instructions`**

- Call when an enabled skill's `name` / `description` clearly matches the task.
- Pass only enabled `skill_id` values.
- After loading, follow the skill body to complete the work.
- Do not reload the same skill unless its body is missing from context.

Example:

```json
{
  "function": {
    "name": "skill",
    "arguments": {
      "method": "load_instructions",
      "skill_id": "my-skill-id"
    }
  }
}
```

**`skill:read_resource`**

- Call only when the skill body points at `references/`, `assets/`, or `scripts/` and the task truly needs that file.
- `path` must be a resource-relative path, e.g. `references/api-guide.md`.
- This tool reads file content only; it does not execute scripts or binaries.

Example:

```json
{
  "function": {
    "name": "skill",
    "arguments": {
      "method": "read_resource",
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
