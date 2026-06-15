# Agent scope rules — layered instructions (roadmap)

> **Status:** P0–P1 implemented; P2–P5 planned.

Pointer aligns with Cursor-style **layered rules**: thin product defaults, user/project/session overrides, explicit priority.

## Implemented

### P0 — Instruction priority + slim scope gate

| Item | Location |
|------|----------|
| Priority stack (conversation → USER RULES → … → agent profile) | `agents/_shared/COMMUNICATION_PUBLIC.md` |
| Short scope gate (contract + mid-task discipline; points to delegation) | `agents/coder/prompts/scope_gate.md` |

### P1 — User Coding Rules

| Item | Location |
|------|----------|
| Persisted field `userCodingRules` | `UserSettings` → `user_settings.json` |
| Runtime merge | `merge_user_platform` → `ModelSettings.user_coding_rules` |
| LLM inject `[USER RULES]` (max 4000 chars) | `user_rules.rs` → `single_agent_prompt.rs` after `[MEMORY]` |
| Settings UI (智能体 tab) | `AssistantSettingsPanel.vue` + `useSettingsDialogForm` |

**vs memory `USER.md`:** profile and identity vs **how to code and bound scope**. User edits rules in settings; agent may still use `memory` target `user` for conversational preferences.

## Planned

### P2 — Project rules

- Discover `{workspace_root}/.pointer/rules/*.md` or `POINTER_RULES.md`
- Inject `[PROJECT RULES]` in cacheable system (cap ~4k chars)
- Reload on workspace root change
- Version in git with the repo

### P3 — Session scope

- Optional per-conversation `sessionScope` text (UI or parsed from user message)
- Inject `[SESSION SCOPE]` via `message_loop_prompts_after` user block (alongside task board)
- Priority below conversation messages, above USER RULES (see COMMUNICATION_PUBLIC)

### P4 — Plan mode (product)

- Composer toggle: implementation not requested → enforce `design_only` scenario
- Runtime deny `file_edit` / `file_write` until user confirms plan
- Deliver must include In scope / Out of scope / phased steps

### P5 — Runtime nudge (optional)

- After tool pass: consecutive cross-layer edits without matching In scope → `UiToast` or host inject
- Complements prompts; does not replace user rules

## Prompt assembly order (cacheable excerpt)

See [`llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md). After agent body and before `[Environment]`:

1. Tools appendix  
2. `[Environment]`  
3. `[MEMORY]` / `[USER PROFILE]` (when enabled)  
4. **`[USER RULES]`** (when `userCodingRules` non-empty)  

Future: **`[PROJECT RULES]`** after USER RULES.

## Cross-platform

- **Desktop + Web:** same `user_settings.json` field and inject path via `pointer-core`.
- Project/session layers must respect workspace root on both entry points when implemented (P2/P3).
