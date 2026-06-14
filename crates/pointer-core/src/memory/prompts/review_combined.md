Review the conversation above for both memory and skills.

## Memory

1. Has the user revealed persona, preferences, personal details, or expectations about how you should behave?
2. Has the user expressed workflow habits or corrections worth remembering next session?

Worth remembering → use the **`memory`** tool.

## Skills

1. Did the user correct your style, workflow, or preferred approach?
2. Did you discover a reusable technique worth capturing?
3. Are loaded skills outdated or missing expected steps?

Worth updating → use **`skill_patch`** for **user-managed** skills under ~/.pointer/skills.
Do **not** patch system bundled skills (`provenance=system`).
Inspect with **`skill_read`**.

Do not encode transient tool failures as permanent skill rules.

If nothing needs saving or patching, reply with exactly: Nothing to save.

You may only call `memory` and skill tools. Do not attempt other tools.
