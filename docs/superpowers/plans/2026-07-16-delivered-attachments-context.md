# Delivered Attachments Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-inject assistant reply `attachments` into later LLM turns via API-only `<!-- pointer-delivered-attachments -->` manifest (symmetric to user uploads).

**Architecture:** Persist still strips `MEDIA:` into structured `attachments`. At `make_openai_messages` time, assistant content gets the same Markdown entry format as users, under a distinct marker, with no needs-intent marker.

**Tech Stack:** Rust (`pointer-core`), existing `media/manifest.rs` + `openai_convert.rs`, agent prompt Markdown.

## Global Constraints

- Marker: `<!-- pointer-delivered-attachments -->` (exact).
- Entry fields: reuse `format_attachment_entry` (fileName / ref / localPath / size / remote rules).
- Never append `<!-- pointer-attachment-needs-intent -->` for delivered files.
- Wire-only: do not write markers into persisted `content` or UI.
- App + Web + IM share `make_openai_messages`.
- Prompt text in English; no filename references inside prompt bodies beyond marker names the model sees.

---

### Task 1: Manifest helpers + unit tests

**Files:**
- Modify: `crates/pointer-core/src/media/manifest.rs`
- Modify: `crates/pointer-core/src/media/mod.rs`

**Interfaces:**
- Produces: `DELIVERED_ATTACHMENTS_MARKER`, `format_delivered_attachments_api_manifest(&[MediaAttachment]) -> String`, `append_delivered_attachments_api_context(content: &str, attachments: &[MediaAttachment]) -> String`

- [ ] **Step 1:** Add failing tests in `manifest.rs` for delivered marker, no needs-intent, empty attachments unchanged.
- [ ] **Step 2:** Implement marker + format/append helpers sharing `format_attachment_entry`.
- [ ] **Step 3:** Export from `mod.rs`.
- [ ] **Step 4:** Run `cargo test -p pointer-core manifest::` and pass.

### Task 2: Wire injection in `make_openai_messages`

**Files:**
- Modify: `crates/pointer-core/src/models/openai_convert.rs`

**Interfaces:**
- Consumes: `append_delivered_attachments_api_context`

- [ ] **Step 1:** Add failing test: assistant with attachments → API content contains delivered marker; caption preserved; no user marker / needs-intent.
- [ ] **Step 2:** In `Role::Assistant`, append delivered context before inserting `content`.
- [ ] **Step 3:** Run `cargo test -p pointer-core make_openai_messages_tests` and pass.

### Task 3: Prompts + design doc

**Files:**
- Modify: `crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md`
- Modify: `crates/pointer-core/src/agents/general/AGENT.md`
- Modify: `crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md` (one line)
- Modify: `docs/design/multimedia-support.md`

- [ ] **Step 1:** Document delivered marker semantics (already delivered; reuse paths; not a new user upload; no intent ask from this block alone).
- [ ] **Step 2:** Note assistant API-only injection in multimedia-support design doc.

### Task 4: Verify + commit

- [ ] **Step 1:** `cargo test -p pointer-core media::manifest openai_convert::make_openai_messages_tests`
- [ ] **Step 2:** Commit implementation (not force-push).
