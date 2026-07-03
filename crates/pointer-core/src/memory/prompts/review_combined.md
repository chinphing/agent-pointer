Review the conversation above for both memory and skills.

## Memory

1. Has the user revealed persona, preferences, personal details, or expectations about how you should behave?
2. Has the user expressed workflow habits or corrections worth remembering next session?

Worth remembering → use the **`memory`** tool.

## Skills

Automatic skill file updates are **disabled**. If loaded skills seem outdated, mention it
briefly — skill edits belong in a **`run_subagent` → coder** handoff with the skill root
as **`workspaceRoot`**, not ad-hoc overwrites from the lead agent.

Do not encode transient tool failures as permanent skill rules.

If nothing needs saving, reply with exactly: Nothing to save.

You may only call `memory`. Do not attempt other tools.
