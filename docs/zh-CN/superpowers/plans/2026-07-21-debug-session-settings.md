# Debug Session Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make debug model configuration immediately usable from memory without ever persisting it to disk.

**Architecture:** Introduce a narrow `DebugSessionSettings` DTO and one Core update boundary that atomically replaces only debug-session fields in the in-memory platform config. APP and WEB expose that boundary directly. The settings dialog snapshots its payload before any awaited save and refreshes the Store only after requests complete.

**Tech Stack:** Rust, Tauri 2, Axum, Vue 3, Pinia, TypeScript, Vitest.

## Global Constraints

- Debug settings must never be written to disk.
- Changes must affect subsequent APP and WEB conversations immediately.
- Application restart restores platform defaults.
- API keys must not appear in logs.
- Do not create a git commit unless the user explicitly requests one.

---

### Task 1: Core in-memory update boundary

**Files:**
- Modify: `crates/pointer-core/src/models/settings.rs`
- Modify: `crates/pointer-core/src/chat_service/app_state.rs`

**Interfaces:**
- Produces: `DebugSessionSettings`
- Produces: `AppState::update_debug_session_settings(DebugSessionSettings) -> anyhow::Result<EffectiveSettingsView>`

- [ ] Add a failing Core test that sets a non-default provider/model and agent/media/computer mappings, calls the new method, and asserts `effective_settings_view()` returns them.
- [ ] Run the focused test and confirm it fails because the DTO/method does not exist.
- [ ] Add the DTO and update method. Clone the current platform config, replace only DTO fields, validate active provider/model references, then atomically replace the in-memory config.
- [ ] Log one info line with counts only.
- [ ] Run the focused test and confirm it passes.

### Task 2: APP and WEB transport

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `server/src/main.rs`
- Modify: `src/types/chat.ts`
- Modify: `src/lib/api.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/web.ts`

**Interfaces:**
- Produces: Tauri command `update_debug_session_settings`
- Produces: WEB `PUT /api/debug-session-settings`
- Produces: frontend `updateDebugSessionSettings(settings)`

- [ ] Add compile-time/route coverage for the new DTO and endpoint.
- [ ] Confirm the focused build/test fails before implementation.
- [ ] Wire both transports to the same Core method and existing platform-access authorization.
- [ ] Ensure neither transport calls `update_agent_settings`, `update_platform_settings`, or persistence helpers.
- [ ] Run focused Rust tests/checks.

### Task 3: Immutable frontend save orchestration

**Files:**
- Modify: `src/stores/settings.ts`
- Modify: `src/components/settings/SettingsDialog.vue`
- Modify: `src/composables/useSettingsDialogForm.ts`
- Test: existing/new Vitest file near settings dialog/store tests

**Interfaces:**
- Produces: `saveDebugSession(snapshot: DebugSessionSettings)`
- Consumes: `updateDebugSessionSettings`

- [ ] Write a failing Vitest regression: choose a non-default debug model, simulate `saveUser` returning old effective settings, then assert the debug request still carries the chosen model.
- [ ] Run the test and confirm the request receives the default model.
- [ ] Build all save payloads synchronously at the start of `saveFromFooter`.
- [ ] Route debug provider/model saves through `saveDebugSession`; remove the multi-request `savePlatform`/`saveSession` workaround for these fields.
- [ ] Refresh effective settings once after each completed API operation without rebuilding payloads from refreshed Store state.
- [ ] Run the regression and related frontend tests.

### Task 4: Documentation and full verification

**Files:**
- Modify: `docs/internals/settings-provider-ui.md`

- [ ] Document the dedicated in-memory boundary and immutable-snapshot rule.
- [ ] Assert serialized `PersistedLocalPlatformSettings` still excludes every debug-session field.
- [ ] Run `cargo test -p pointer-core` focused settings tests.
- [ ] Run relevant server/Tauri checks.
- [ ] Run `npm test`.
- [ ] Run `npm run build`.
- [ ] Inspect IDE lints for modified files and fix introduced diagnostics.
