# Deliver 通道绑定 P3a Implementation Plan

> **For agentic workers:** Implement task-by-task. Steps use checkbox syntax.

**Goal:** Auto-bind last DM per channel; cron `deliver` uses channel names; Automation UI is toggle + channel checkboxes.

**Architecture:** Inbound DM updates `homeRecipientId` via ChannelGateway; `cron_job` validates bindings before persist; UI maps channel multi-select ↔ deliver string.

**Tech Stack:** Rust (`pointer-channels`, `pointer-core`), Vue Automation panel, existing `ChannelsConfig`.

## Global Constraints

- Prompt md: English, no filenames, short lines.
- UI copy: concise Chinese, no eng jargon / raw IDs in primary UI.
- APP + Web both; Mac/Win/Linux via shared crates.
- No silent failures on bind write / deliver validation.

---

## Task 1: Auto-bind last DM + optional display name

**Files:** `config.rs`, new or `im_delivery`/`gateway` helper, `dispatch.rs` (or inbound after parse)

- [ ] Add `homeDisplayName` to `ChannelAccountConfig`
- [ ] `set_channel_home_binding(channel, account_id, recipient_id, is_group, display_name)`
- [ ] On inbound **DM only**, update binding to sender (last DM wins); log info
- [ ] Unit tests: DM updates; group does not

## Task 2: Validate deliver for cron create/update

**Files:** `im_delivery.rs` (`validate_deliver_channels`), `cron_job/mod.rs`, HTTP create/update if needed

- [ ] Reject unbound channel names / empty `all`
- [ ] Normalize channel names to lowercase
- [ ] Update `cron_job.md` prompt (channel names only on happy path)
- [ ] Tests for validate helper

## Task 3: Automation UI — toggle + channel multi-select

**Files:** `AutomationSettingsPanel.vue`, types if needed

- [ ] Replace free-text primary UX with push toggle + bound channel checkboxes
- [ ] Map selection ↔ `deliver` string; single bound channel auto-checked on create
- [ ] Show unbound hint; list display uses channel labels

## Task 4: Docs + verify

- [ ] Update `channel-integration.md` Phase 3a done notes
- [ ] `cargo test` im_delivery + cron paths; `cargo check` server/tauri
