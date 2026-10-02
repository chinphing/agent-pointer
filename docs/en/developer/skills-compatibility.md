# Skills ecosystem compatibility (Codex / Agent standard)

English | [简体中文](../../zh-CN/developer/skills-compatibility.md)

Pointer uses the community-standard **`SKILL.md`** format (YAML frontmatter + Markdown body) and is compatible with Agent-standard directory layouts such as OpenAI Codex and `.agents/skills`. It does not scan Cursor (`.cursor/skills`) or Claude Code (`.claude/skills`) automatically, to avoid interfering with the skills of other IDEs on the machine.

## Auto-discovery paths

On `reload_meta` / startup load, the following directories are scanned for subfolders (containing `SKILL.md` or `skill.md`). **For skills with the same name, the one scanned first wins** (priority high to low):

| Priority | Path | Source | `provenance` | How to modify |
|--------|------|------|--------------|----------|
| 1 | `~/.pointer/skills/` | User library (imported / created by an Agent) | `user` | **`run_subagent` → coder** (`file_*`); **`skill_import`** installs whole packages only |
| 2 | `~/.agents/skills/` | User's Codex / Agent standard directory | `external` | ❌ |
| 3 | `{data_dir}/PointerApp/skills/` (or `{POINTER_APP_DATA_DIR}/skills/` in standalone) | The app's bundled synced copy | `system` | ❌ |

**Not scanned**: the workspace `{workspace}/.agents/skills/`, the workspace `./skills/` (Pointer's built-in skill source), and vendor directories starting with `.`.

**pointer-server built-in source (for install sync only; not used directly as a runtime scan root)**: `POINTER_SERVER_SKILLS_DIR` / TOML `[server].skills_dir` → `skills/` next to the executable → `{exe}/../../skills` → `{cwd}/skills` → `/usr/share/pointer-server/skills` on the Linux deb. On startup it is synced to `{app_data_dir}/skills/` and then loaded via row 3 of the table above.
**Modifying an external skill**: use **`skill_import`** to copy it into `~/.pointer/skills/`.
**Updating an existing user skill**: for the general lead's delegation rules see the **`coder`** section of the `run_subagent` tool prompt (the single source of truth); a typical **`workspaceRoot`** is `~/.pointer/skills/{name}/`.

## Frontmatter compatibility

| Field | Notes |
|------|------|
| `name` | Required, the skill id |
| `description` | Required, supports `>` / `|` multi-line YAML |
| `allowed-tools` / `allowed_tools` | Optional; recorded as `toolNames` metadata (not forcibly registered as Pointer tools) |
| `tags` / `metadata.tags` | Optional; merged into the UI tags |
| `license` / `compatibility` | Optional; the length is validated, not used at runtime |
| Other fields (`version`, `triggers`, `disable-model-invocation`, etc.) | Ignored, no error |

## Resource directory

Consistent with Codex / OpenClaw / Hermes, files such as `references/`, `scripts/` and `assets/` in the same directory are supported; they are read on demand via **`skill_read`** (`path` required) (scripts are not executed automatically). When reading instructions use `path=SKILL.md`; when reading resources pass a skill-relative path.

**`skill_read` reads the disk live** (the same shape as Hermes' `skill_view`): `path` is required and must stay inside the skill root directory (`..` traversal is forbidden).

- **`path=SKILL.md`**: re-reads the body from disk every time; it does **not** use the body cached in the registry. After editing the file, no `reload_meta` / restart is needed for it to take effect.
- **Resource paths** (such as `references/`): also read from disk every time; the whole skill tree is **not** indexed into `resourceFiles` at startup scan time (to avoid venvs / cases and the like pushing `/api/skills` to several MB).

The registry and `/api/skills` keep only directory metadata (id, name, description, tags, toolNames, source, provenance, etc.). After an Agent adds a reference or edits `SKILL.md` with `file_*`, `skill_read` works without a restart.

**Note**: the description in `<available_skills>` still comes from the last scan; after changing a frontmatter description you must refresh the skill list for the directory summary to update.

The directory contents = the current lead's resolution result: `agentSkillOverrides[lead] ?? defaultSkillIds`, then merged with the missing bundled defaults (see skills-persistence). The entry point no longer passes a skill list; if a built-in skill is still not in the list, the model will not trigger it when scanning by description.

## Runtime injection (OpenClaw-aligned)

When skills are enabled, the system injects the **`<available_skills>`** catalog, where each skill contains:

| Field | Meaning |
| --- | --- |
| `<name>` | Skill id; the **`skill_id`** for **`skill_read`** |
| `<description>` | The frontmatter summary |
| `<location>` | The `SKILL.md` path (home / app data are shown as `~/…`) |

**`skill_read` requires `path`**: pass `SKILL.md` to read instructions; pass a path relative to the skill root to read a resource.

The skill root directory = **`dirname(<location>)`**. `{baseDir}` in the body is replaced with an absolute path when **`skill_read`** loads it; when running scripts with `terminal`, use absolute paths.

## Manual import

The skill library UI or **`skill_import`** can still install a zip / local directory into **`~/.pointer/skills/`**; after installation it overrides the external / bundled load result with the same id.

## Runtime scope

Skills are loaded under the **general** / **coder** lead agents; for how users enable them see [`../user/skills.md`](../user/skills.md); for persistence details see [skills-persistence.md](../../zh-CN/developer/skills-persistence.md).

## Cross-platform

macOS / Windows / Linux use the same path conventions (`~` is the user's home directory, and `CODEX_HOME` can override the Codex root directory).
