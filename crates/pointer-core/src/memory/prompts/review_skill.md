Review the conversation above and consider updating Skills if appropriate.

Focus on:

1. Did the user correct your style, workflow, or preferred approach?
2. Did you discover a reusable technique worth capturing for similar tasks?
3. Are skills loaded this session outdated or missing steps the user expects?

When updating:

- Prefer patching skills already loaded in this session.
- Add or refine `references/` files before creating a brand-new skill.
- Do not encode transient failures (e.g. a tool was down once) as permanent skill rules.

If a **user-managed** skill under ~/.pointer/skills should change, use **`skill_patch_instructions`** with the full updated SKILL.md body (markdown after frontmatter).
Do **not** patch **system bundled** skills (provenance=system under the app data directory).

To inspect skills, use **`skill_load_instructions`** and **`skill_read_resource`**.

If nothing should change, reply with exactly: Nothing to save.

You may only call skill tools listed above. Do not attempt other tools.
