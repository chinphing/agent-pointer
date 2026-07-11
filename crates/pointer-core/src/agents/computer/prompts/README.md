# Computer agent prompts (maintainer index)

Runtime slices are merged in code (`computer_communication_for_tier`, `computer_agent_body_for_tier`).
**All tiers (Primary / Intermediate / Advanced) use the same Primary prompt text**; only the per-tier LLM model differs at runtime.

The model only sees the merged system text — not these paths.

Slot labels (`[Screen after action]`, …) are defined in `vision/screen_overlay.rs` and injected **immediately before each image** in the `[CUR_SCREEN]` user message (see `extension_hooks/screen_inject.rs`). All tiers use the Primary 2–3 image layout.

## Layout

```
prompts/
  tiers/
    primary/       communication.md + loop.md   ← runtime source for all tiers
  os/              macos.md | windows.md | linux.md
  ui_disabled_controls.md   shared — gray/disabled controls (all tiers)
  modules/verify/  host post-execute verify LLM prompts (not in main system merge)
author/            not loaded (authoring reference only)
AGENT.md           manifest + config (repo root of `computer/`)
```

## Edit guide

| Change | File |
|--------|------|
| Role / turn loop / queue board (all tiers) | `tiers/primary/communication.md`, `tiers/primary/loop.md` |
| OS-specific hints | `os/*.md` |
| Disabled UI controls | `ui_disabled_controls.md` |
| Host verify LLM (internal) | `modules/verify/*.md` |
