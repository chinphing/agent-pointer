### `skill`

Unified Skill progressive-disclosure tools (same pattern as **`file:method`** / Computer **`mouse:method`**). Prefer qualified XML names: **`skill:load_instructions`**, **`skill:read_resource`** — the runtime merges them into `tool_name` **`skill`** plus **`method`**. You may also call **`skill`** with a top-level **`method`** field.

#### Usage

Progressive disclosure for enabled Skills.

**`skill:load_instructions`** (or `skill` + `method`: `load_instructions`)

- Call when an enabled skill’s `name` / `description` clearly matches the task.
- Pass only enabled `skill_id` values.
- After loading, follow the skill body to complete the work.
- Do not reload the same skill unless its body is missing from context.

**`skill:read_resource`** (or `skill` + `method`: `read_resource`)

- Call only when the skill body points at `references/`, `assets/`, or `scripts/` and the task truly needs that file.
- `path` must be a resource-relative path, e.g. `references/api-guide.md`.
- This tool reads file content only; it does not execute scripts or binaries.
- Do not read unrelated resources.

#### Methods

| `method` | Purpose |
|----------|---------|
| `load_instructions` | Load the full **`SKILL.md`** body for an enabled Skill (layer 2). |
| `read_resource` | Read one indexed resource file under that Skill (layer 3). |

#### Parameters

- **`method`** — Required unless using a qualified name (`skill:load_instructions`, …). One of: `load_instructions`, `read_resource`.

**`load_instructions`**

- **`skill_id`** — The Skill’s id (must be enabled for this session).

**`read_resource`**

- **`skill_id`** — Same as above.
- **`path`** — Resource-relative path listed in the Skill index, e.g. `references/api-guide.md`.

#### XML examples

Only when the **`skill`** tool is available to you and the session lists the skill. `skill_id` must match an **enabled** skill for this session.

**`skill:load_instructions`**

```xml
<response>
  <thoughts>Task matches an enabled skill; load its full SKILL.md.</thoughts>
  <headline>Load skill</headline>
  <tool_name>skill:load_instructions</tool_name>
  <tool_args>
    <skill_id>your-enabled-skill-id</skill_id>
  </tool_args>
</response>
```

**`skill:read_resource`** — include **`path`** (resource-relative, e.g. `references/guide.md`) alongside **`skill_id`**.

```xml
<response>
  <thoughts>Need an asset from the skill bundle.</thoughts>
  <headline>Read skill resource</headline>
  <tool_name>skill:read_resource</tool_name>
  <tool_args>
    <skill_id>your-enabled-skill-id</skill_id>
    <path>references/guide.md</path>
  </tool_args>
</response>
```
