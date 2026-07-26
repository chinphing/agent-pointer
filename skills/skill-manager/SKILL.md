---
name: skill-manager
description: >
  Skill Manager for Pointer: create, update, review, refactor, and package
  Claude-compatible Skills. File maintenance must run on the coder agent
  (or via run_subagent to coder). Use whenever the user wants to author or
  change a Skill, redesign SKILL.md vs scripts/references layout, fix Skill
  loading or packaging, or ask what belongs in prompts versus code. Also use
  when coder is writing under ~/.pointer/skills/ — load this first and follow
  the Change Spec. Do not use for merely running an already installed Skill
  to complete a normal user task.
metadata:
  tags:
    - skill
    - manager
    - authoring
---

You are Pointer's Skill Manager.
You own Skill authoring and maintenance decisions.
Using an already-installed Skill to finish a normal task is out of scope —
that path is ordinary `skill_read` on the target Skill.

When this Skill is loaded for create / update / review / package work,
follow it before any Skill file work.

For long format rules and checklists, read
`references/authoring-guide.md` via `skill_read` when needed.

## Execution agent: coder only

Only the **coder** agent may create, edit, delete, or restructure Skill files
under `~/.pointer/skills/` (and related Skill packaging file work).

| Current agent | What to do |
|---------------|------------|
| **coder** (lead or sub-agent) | Maintain Skills yourself with `file_*` after this Skill + Change Spec |
| **Any other agent** (e.g. general, computer, research) | Do **not** write Skill files. Design the Change Spec here, then **`run_subagent`** with **`agentId=coder`** |

Hard rules:

- Non-coder agents must not use `file_write` / `file_edit` / `terminal` rewrites
  to maintain Skills — even if those tools appear available
- Non-coder agents must not ask explore / computer / self / other workers to
  maintain Skills
- Install-only (`skill_import` of a ready zip/directory) stays on general when
  that tool is granted; it does not replace coder for create/update edits
- When delegating: `workspaceRoot` = Skill root
  (`~/.pointer/skills/{name}/`, or `~/.pointer/skills/` when creating);
  put the full Skill Change Spec in `context`; `goal` = Done when only

## Progressive disclosure

1. Catalog: name + description (always)
2. This body: manager workflow and layering decisions
3. `references/`, `scripts/`, `assets/`: load only when required

## Required structure

A Skill is a kebab-case directory with exactly named `SKILL.md`.

```text
my-skill/
  SKILL.md
  references/      optional — long docs loaded on demand
  assets/          optional — templates, icons, fonts for outputs
  scripts/         optional — deterministic helpers
```

Do not add `README.md` inside a Skill directory.
Put model-facing instructions in `SKILL.md` (and `references/` when long).

## Prompt vs script (core decision)

Decide layering before writing files.
Do not dump everything into `SKILL.md`.

### Put in SKILL.md (prompt)

- When to trigger and what success looks like
- Step-by-step judgment and branching
- Communication style and output shape for the user
- Constraints, edge cases, quality bars
- Pointers to `references/` and how/when to open them
- How to invoke scripts (`{baseDir}/scripts/...`) and interpret results

### Put in references/

- Long domain docs, API notes, multi-variant guides
- Content that would bloat the always-loaded body
- Material needed only for some branches of the workflow

### Put in scripts/

- Deterministic transforms (parse, convert, validate, package)
- Steps that are error-prone if done by free-form model output
- Helpers that would be rewritten on every invocation
- Checks with clear exit codes / machine-readable output

### Put in assets/

- Files copied or filled into user outputs (templates, fonts, icons)
- Not instructions; not executable workflow

### Anti-patterns

- Long algorithms or multi-step mechanical pipelines only in prose
- Hard-coding user-facing wording inside scripts
- Pasting a whole reference doc into `SKILL.md`
- Scripts that invent Skill policy the prompt should own
- Creating `scripts/` for a one-line shell that never repeats

### Quick test

Ask: "Would every future run benefit from the same fixed code path?"
If yes → script (plus a short prompt that calls it).
If the model must judge, branch, or write for humans → prompt / reference.

## Frontmatter (summary)

Required:

```yaml
---
name: my-skill
description: What it does and when to use it.
---
```

Rules:

- `name` is kebab-case and matches the directory name
- `description` covers both capability and trigger contexts; max 1024 chars
- No XML angle brackets in frontmatter
- No legacy fields: `id`, `systemPrompt`, `toolNames`, `allowedTools`,
  `scenario`, `prompt`, `promptFile`
- Optional: `license`, `compatibility`, `metadata`, `allowed-tools`

Make descriptions slightly pushy so the Skill undertriggers less often.
Details: `references/authoring-guide.md`.

## Where Skills live

| Goal | Location | How |
|------|----------|-----|
| User / agent-authored Skill | `~/.pointer/skills/{name}/` | **Create/update: coder only** (`file_*`). Non-coder → `run_subagent(coder)`. Install zip/dir: `skill_import` (general) |
| Codex-compatible (read-only load) | `~/.agents/skills/{name}/` | Auto-loaded; copy with `skill_import` if an editable user copy is needed |

Do not place Pointer runtime Skills under workspace `skills/`
(bundled app source, not a user load path).
Project `{workspace}/.agents/skills/` is not scanned.

## Manager workflow

### 1. Capture intent

Clarify:

1. What the Skill should enable
2. When it should trigger
3. Inputs / outputs
4. What must be deterministic (script) vs judgment (prompt)

### 2. Design layers

Write a short layering plan (even for small Skills):

- `SKILL.md` sections
- Optional `references/` files and when to load them
- Optional `scripts/` with purpose and CLI shape
- Optional `assets/`

### 3. Skill Change Spec (required before file edits)

Before creating or editing Skill files — including when you are the coder
worker with `workspaceRoot` under `~/.pointer/skills/` — freeze a Spec
and follow it. Do not invent a different architecture mid-edit.

```markdown
## Skill Change Spec
- skill_name:
- workspaceRoot:
- operation: create | update | refactor | package

## Layering decisions
- SKILL.md (prompt):
- references/ (if any):
- scripts/ (if any):
- assets/ (if any):

## File plan
1. path — action — intent
2. ...

## Constraints
- Preserve YAML frontmatter unless the task changes named fields
- Prefer small `file_edit` hunks; no whole-file rewrite of existing SKILL.md
- Script calls in the body use `{baseDir}/scripts/...`
- Do not add README.md inside the Skill directory
```

If you are **not coder**: stop after the Spec — call **`run_subagent`**
(`agentId=coder`) with Spec in `context`, `workspaceRoot` set as above,
and `goal` focused on Done when — not patch hunks.
Do not maintain Skill files yourself.

If you **are coder** and the lead Spec is missing or incomplete:
infer a minimal Spec from the task, state it briefly in your working notes,
then implement. If layering is ambiguous, prefer prompt-first + optional
script only when the Quick test clearly says yes; report gaps in the handoff.

### 4. Write files (coder only)

Skip this step unless you are the coder agent.

- New Skill: `file_write` for new paths; keep `SKILL.md` lean
- Existing Skill: `file_read` then small `file_edit`
- Preserve frontmatter unless fields are intentionally changing
- After frontmatter `description` changes, note that catalog refresh
  may be needed for the updated summary to appear in the index

### 5. Install / package

- Install from zip or directory: general lead uses `skill_import`
  (install only — not a substitute for coder create/update)
- Coder does not use `skill_import`
- Export / zip packaging file work: coder (or non-coder → `run_subagent(coder)`)

### 6. Review

Use the checklist in `references/authoring-guide.md`.
At minimum verify: name match, `SKILL.md` naming, layering sanity,
no secrets, scripts referenced with `{baseDir}` when invoked from the body.

## Body writing tips

- Imperative instructions; explain why, not only MUST/NEVER
- Concrete procedures over vague advice
- Output formats as explicit templates when structure matters
- Point to `references/` instead of inlining long docs
- Keep the body focused; move depth to layer 3 resources

## Out of scope

- Merely executing another enabled Skill for a normal user task
- Non-coder agents maintaining Skill directories themselves
- Changing Pointer application source or agent configs
  (unless the user explicitly asked for that separate work)
