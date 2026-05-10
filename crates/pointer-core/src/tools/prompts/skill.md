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
