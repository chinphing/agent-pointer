Review the conversation above and consider whether any Skills should be updated.

Automatic skill file updates are **disabled** in background review.

If skills seem outdated, note it in your reply — the **general** lead should delegate
**`run_subagent(agentId="coder")`** with **`workspaceRoot`** = `~/.pointer/skills/{name}/`
for file changes (not **`skill_import`** re-install unless installing a new package).

Do not encode transient failures (e.g. a tool was down once) as permanent skill rules.

If nothing is worth noting, reply with exactly: Nothing to save.

You may only call tools listed above. Do not attempt other tools.
