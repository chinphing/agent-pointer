# skills tools

Use to load the `SKILL.md` body for a given skill (progressive disclosure layer 2).

Rules:
- Call when an enabled skill’s `name` / `description` clearly matches the task.
- Pass only enabled `skill_id` values.
- After loading, follow the skill body to complete the work.
- Do not reload the same skill unless its body is missing from context.

Use to read a skill resource file (progressive disclosure layer 3).

Rules:
- Call only when the skill body points at `references/`, `assets/`, or `scripts/` and the task truly needs that file.
- `path` must be a resource-relative path, e.g. `references/api-guide.md`.
- This tool reads file content only; it does not execute scripts or binaries.
- Do not read unrelated resources.
