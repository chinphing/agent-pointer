# Cron One-Shot Soft Complete Implementation Plan

> **For agentic workers:** Implement task-by-task. Spec: `docs/superpowers/specs/2026-07-21-cron-oneshot-soft-complete-design.md`

**Goal:** Hermes-style one-shot delays (`30m`, ISO) with soft-complete (disable, keep row).

**Architecture:** `ParsedSchedule` enum; `schedule_kind`/`schedule_raw` columns; `mark_ran` branches on kind; reject re-enable once.

**Tech Stack:** Rust pointer-core, SQLite migration, Vue Automation UI (minimal).

## Global Constraints

- Soft complete: never delete on fire
- Once re-enable: reject
- Cross-entry: tool + API share parse
- Prompt English; UI concise Chinese

### Task 1: Schedule parse
### Task 2: DB + mark_ran / set_enabled
### Task 3: Tool + prompts + AGENT.md
### Task 4: API/Tauri create path
### Task 5: UI picker + list status
### Task 6: Tests + docs
