### Scenario: skill_change

**Classify** when creating or updating a user Skill
(`workspaceRoot` under `~/.pointer/skills/`, or the task is clearly
Skill authoring / prompt↔script redesign).

Skills are **not** ordinary app code: behavior lives in **two surfaces** —
model-facing prompts and deterministic scripts. Do **not** treat Impact
as call-chain only.

**Orient (before first edit):**

1. **`skill_read`** Skill Manager if not loaded — freeze a **Skill Change Spec**
   (authority for layering and File plan).
2. Answer internally (do not paste to the user):
   - **Who decides?** model judgment vs script computation
   - **What stays deterministic?** → `scripts/`
   - **What stays model-facing?** → `SKILL.md` / `references/`
3. **Dual-surface default:** a behavior change reviews **both** prompt and
   script sides. Mark a side **N/A** only with a one-line reason
   (e.g. "trigger wording only; CLI unchanged").

**Change:** follow the Spec File plan; prefer small `file_edit` hunks;
preserve `SKILL.md` frontmatter unless the task changes named fields.
Script invocations in prompts use `{baseDir}/scripts/...`.

**Check:**

- Script side: run the relevant CLI / tests when scripts exist or changed.
- Prompt side: confirm call paths and field contracts still match the scripts.
- Skip a side only with the same N/A reason as Orient.

**Deliver:** list prompt paths changed, script paths changed, and any
N/A side with reason. Do not claim "skill updated" after editing only one
surface unless the other was explicitly N/A.
