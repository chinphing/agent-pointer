# Using Skills

English | [简体中文](../../user/skills.md)

Pointer injects reusable capability instructions (translation, document handling, domain workflows, …) into conversations through **Skills**.

## Skill library

1. Open the **Skills library**
2. Tick the skills you want to enable; the state is stored in user settings and survives a restart
3. New users have the repository's built-in skills enabled by default (find skills, dev environment, Skill manager, Pointer management, browser automation). The Office / PDF skills must be imported yourself

Once enabled, the **general** / **coder** agents load the matching skill index in the conversation; when the model needs it, it reads the body and resources through `skill_read` (required `path`: `SKILL.md` for the instructions, a relative path for resources).

Merely dropping a skill folder into `~/.pointer/skills/` does **not enable it automatically**. You must tick it in the skill library, or have the assistant import and enable it.

## Importing a zip

1. Skill library → **Import zip**
2. Each Skill must be a **kebab-case directory** containing an exactly named **`SKILL.md`**
3. It is imported into `~/.pointer/skills/` and the list refreshes automatically

`skill.md`, `skill.json` or a root `manifest.json` are not supported.

## First launch: importing from other tools

On first launch, if Skills are found in directories such as Codex / Claude / OpenClaw / Hermes, a dialog asks whether to **import them in one click** into the Pointer user library. After you decline or finish, it will not ask again.

## Editing skill content

- Skills in the user library `~/.pointer/skills/` can be edited by the agent by delegating to the **coder** sub-agent
- Built-in skills and the app's bundled library are read-only

## Format and compatibility

For the Skill directory structure, frontmatter fields and compatibility rules with Codex / `.agents/skills`, see **[`../../developer/skills-compatibility.md`](../../developer/skills-compatibility.md)**.

For implementation details of persistence and load scope see **[`../../developer/skills-persistence.md`](../../developer/skills-persistence.md)** (for integrators and maintainers).
