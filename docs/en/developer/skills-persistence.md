# Skills enablement state and load scope

English | [简体中文](../../zh-CN/developer/skills-persistence.md)

## Directories and mutability (Hermes-aligned + Pointer extensions)

| Layer | Path | Source | How to modify |
|------|------|------|----------|
| **User library** | `~/.pointer/skills/` | `skill_import`, external one-click import, coder sub-agent edits | **`run_subagent` → coder** (`file_*`); **`skill_import`** installs only |
| **Compatibility library** | `~/.agents/skills/` | Codex / Agent standard directory (read-only load) | ❌ (`skill_import` copies into the user library, after which coder can edit) |
| **System library** | `{data_dir}/PointerApp/skills/` | App bundled sync | ❌ (protected by `.bundled_manifest`) |

Runtime load order: **user library** → **`~/.agents/skills`** → **system library**; for the same id, whichever is scanned first wins.

System library sync rules (same shape as Hermes):

- On install/update, write `{data_dir}/skills/.bundled_manifest` (`name:sha256`)
- Local hash **matches** the manifest → a new version may be pulled
- **Mismatch** → treated as modified, overwrite skipped

User library metadata: `~/.pointer/skills/.usage.json` (`agentCreated`, `pinned`, etc.).

## First launch: external Skills probe

**First time only** (when `~/.pointer/external_skills_probe_done` does not exist) scan:

| Source id | Directory |
|---------|------|
| codex | `$CODEX_HOME/skills` or `~/.codex/skills` |
| claude | `~/.claude/skills` |
| openclaw | `~/.openclaw/skills` |
| hermes | `~/.hermes/skills` |

If there are Skills not yet loaded into Pointer, the client/Web shows a dialog asking whether to **one-click import** them into `~/.pointer/skills/`. On refusal or after the import completes, a marker is written and the probe is not repeated.

**Note**: at runtime external directories such as Codex/Hermes are **no longer** mounted automatically; they enter the user library only through the first import or a manual `skill_import`.

## Single source of enablement: `agentSkillOverrides`

Enabled skills are stored per agent in **`user_settings.json`** under `agentSkillOverrides` (`agentId → skill ids`).

| Action | Behavior |
|------|------|
| Ticking/unticking in the skill library | `skills.toggleForAgent` → `saveUser({ agentSkillOverrides })` |
| App start | after `settings.load`, `skills.initEnabledFromUserSettings()` (migrates legacy fields only) |
| `skill_import` + `auto_enable` | writes the current lead's `agentSkillOverrides[leadId]` and pushes `skills_updated` |
| External one-click import | `import_external_skills` → user library + `reload_meta` |

The legacy field `enabledSkillIds` **no longer takes part in runtime resolve**; if it exists and there is no `general` override yet, it is migrated to `agentSkillOverrides.general` at startup.

`Conversation.skillIds` is a historical field and is **no longer** a source of enablement state.

## On disk ≠ enabled

| Layer | Location | Role | Who writes |
|----|------|------|--------|
| File | `~/.pointer/skills/{name}/` | `skill_read` resolves from disk (does not consult the enablement list) | coder `file_*`, or `skill_import` |
| Enablement list | `agentSkillOverrides[leadId]` | Enters `<available_skills>`, automatic matching | `skill_import(auto_enable)` or ticking in the settings page |

Both layers are required before automatic matching applies. Re-importing a zip overwrites the user library copy and counts as `imported`. The override performs no existence check; ids that are not installed are marked "not installed" in the index.

## Runtime resolution (the only chain)

```
effective = agentSkillOverrides[agentId] ?? defaultSkillIds
effective = merge missing bundled ids from defaultSkillIds
```

`accessPolicy.allowSkills` / `denySkills` are legacy fields and are **no longer filtered at runtime**. Skill boundaries are decided only by `defaultSkillIds` + user overrides.

Entry points (APP / Web / IM / Cron / Webhook) **do not carry** a skill list; when overrides are empty, `run_chat` loads them from `user_settings.agentSkillOverrides`.

- **Single agent**: when the lead is **`general`** / **`coder`**, injection follows the formula above.
  **general** may use **`file_*`** + **`skill_import`**; for when to write locally versus
  **`run_subagent(coder)`** → see the `run_subagent` tool prompt **`coder`** subsection
  (the only source). **coder** only has **`skill_read`** + **`file_*`**.
- **Sub-agent**: **coder** uses its own `defaultSkillIds` (through the same chain); a **self fork** uses `inheritsFromParent` (the parent's resolved list). Other sub-agents usually do not load skills.

Upgrade completion: during resolve, ids in the agent's `defaultSkillIds` that are still bundled are merged into a stale override (for example, an old coder override missing **`skill-manager`**). The frontend `ensureSystemSkillsEnabled` only performs the same logic as **persistence**, so that settings-page ticks match runtime.

## Curator and self-improvement

| Mechanism | Scope | Trigger |
|------|------|------|
| **Self-improvement review (P3)** | Memory / user profile | `memoryNudgeInterval` (automatic skill rewriting is disabled) |
| **Curator (P5)** | User library stale marking / archiving | Periodic (no LLM rewriting) |

System library skills have `provenance: "system"`, `mutable: false` in the API. `.agents/skills` sources have `provenance: "external"`, `mutable: false`.

## Built-in skills

The repository `skills/` is packaged with the app; at startup it is synced to **`{data_dir}/PointerApp/skills/`** and registered in the manifest.

**Agent defaults**: see `defaultSkillIds` in each agent's `AGENT.md` (aligned with bundled). `general` includes all built-in skills by default.

**Office skills (docx / xlsx / pptx / pdf)** are not packaged with the repository; install them on demand from upstream
[anthropics/skills](https://github.com/anthropics/skills) (MIT, with `SKILL.md` and `scripts/`)
— see `skills/find-skills`. Do not hand-write a trimmed-down version in this repository, and do not sync it into the packaged directory `skills/`.

## Implementation entry points

- `crates/pointer-core/src/agents/mod.rs` — `resolve_skill_ids` / `sub_agent_skill_ids`
- `crates/pointer-core/src/chat_service/session.rs` — load overrides from settings
- `crates/pointer-core/src/skills/provenance.rs` — manifest, mutable determination
- `crates/pointer-core/src/skills/external.rs` — dual-directory load, bundled sync
- `crates/pointer-core/src/skills/external_probe.rs` — first external probe and import
- `crates/pointer-core/src/skills/curator.rs` — user library housekeeping
- `src/components/skills/ExternalSkillsImportModal.vue` — first-import dialog
