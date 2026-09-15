# Skill authoring guide

Load this when you need full format rules, examples, or the final checklist.
The Skill Manager body owns workflow and prompt-vs-script decisions.

## Frontmatter

Every `SKILL.md` starts with YAML frontmatter.

```yaml
---
name: my-skill
description: Explains what the Skill does and when to use it.
---
```

Rules:

- `name` must be kebab-case and match the directory name
- `description` must explain both what the Skill does and when to use it
- `description` must be concise and no longer than 1024 characters
- Frontmatter must not contain XML angle brackets
- Do not use legacy fields such as `id`, `systemPrompt`, `toolNames`,
  `allowedTools`, `scenario`, `prompt`, or `promptFile`
- Optional official fields: `license`, `compatibility`, `metadata`,
  `allowed-tools`

Tags via metadata when useful:

```yaml
metadata:
  tags:
    - writing
    - review
```

`allowed-tools` only when the Skill truly needs specific tools:

```yaml
allowed-tools: "WebFetch Bash(python:*)"
```

### Description style

First-layer discovery uses only name + description.
Prefer slightly pushy trigger coverage so the Skill is consulted when useful.

Weaker:

```text
How to build a simple dashboard for internal metrics.
```

Stronger:

```text
Build a simple dashboard for internal metrics. Use whenever the user
mentions dashboards, data visualization, internal metrics, or displaying
company data — even if they never say the word dashboard.
```

## Body structure

After frontmatter, include:

1. Role and scope
2. When to use (brief; deep trigger text stays in description)
3. Step-by-step workflow
4. Expected output format
5. Constraints, edge cases, quality checks
6. Links to `references/`, `assets/`, or `scripts/` only when needed
7. Script invocations with `{baseDir}/scripts/...`

Prefer imperative voice.
Explain why a step matters so the model can adapt instead of rote-following.

### Output format example

```markdown
## Report structure
ALWAYS use this exact template:
# [Title]
## Executive summary
## Key findings
## Recommendations
```

### Examples pattern

```markdown
## Commit message format
Example 1:
Input: Added user authentication with JWT tokens
Output: feat(auth): implement JWT-based authentication
```

## Layering examples

### Prompt-only Skill

Use when the work is judgment, writing, or a short procedure with tools
the host already provides.

```text
review-notes/
  SKILL.md
```

### Prompt + references

Use when multiple domains or long docs would bloat the body.

```text
cloud-deploy/
  SKILL.md
  references/
    aws.md
    gcp.md
    azure.md
```

`SKILL.md` selects the variant; the model reads only the needed reference.

### Prompt + scripts

Use when transforms must be repeatable and exact.

```text
csv-normalize/
  SKILL.md
  scripts/
    normalize.py
```

`SKILL.md` states when to run the script, arguments, and how to handle
failure. The script owns parsing and rewriting.

### Full layout

```text
report-pack/
  SKILL.md
  references/
    style-guide.md
  scripts/
    build_report.py
  assets/
    cover-template.docx
```

## Progressive disclosure sizes (guidance)

- Metadata: always in context (~100 words)
- `SKILL.md` body: keep lean; under ~500 lines when practical
- Bundled resources: unlimited; load on demand
- For large reference files, add a short table of contents at the top

## Install and edit paths (Pointer)

| Goal | Location | How |
|------|----------|-----|
| User / agent-created Skill | `~/.pointer/skills/{name}/` | Create/update: coder. Enable: general `skill_import` |
| Codex-compatible Skill | `~/.agents/skills/{name}/` | Read-only load; `skill_import` to copy into the user library |

Do not put Pointer runtime Skills under workspace `skills/`.
Project `{workspace}/.agents/skills/` is not scanned.

Only **coder** may maintain Skill files.
Other agents design the Change Spec, then delegate; they must not edit
Skill paths themselves (no explore / computer / self substitute).

Edit discipline (coder):

- Prefer small, uniquely matchable `file_edit` hunks
- Preserve YAML frontmatter unless the task changes those fields
- Avoid whole-file `file_write` on an existing `SKILL.md`
- After changing frontmatter description, catalog text may need a metadata
  refresh to update the available-skills index

## Safety

Skills must not include malware, exploit code, or instructions that enable
unauthorized access or data exfiltration.
Contents should match the described intent.
Roleplay-style Skills are fine when clearly non-malicious.

Never ship secrets, credentials, or private user data inside a Skill.

## Review checklist

Before finalizing:

- [ ] Directory name and frontmatter `name` match
- [ ] `SKILL.md` is uppercase and exactly named
- [ ] Frontmatter uses only official fields
- [ ] Description is specific enough for first-layer discovery
- [ ] Body is actionable; long depth lives in `references/` when needed
- [ ] Prompt vs script split passes the Quick test in the manager body
- [ ] Script invocations use `{baseDir}/scripts/...`
- [ ] No `README.md` inside the Skill directory
- [ ] No secrets, credentials, private data, or unsafe instructions
- [ ] New skills: lead `skill_import` succeeded (`imported`) or Settings enable
