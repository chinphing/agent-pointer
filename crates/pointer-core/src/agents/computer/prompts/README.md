# Computer agent prompts (maintainer index)

Runtime slices are merged in code (`computer_communication`, `computer_agent_body`).
**One model per performance mode (`agentModeLlm["computer"][mode]`) drives the runtime**, same as every other agent; the prompt text is a single Primary slice.

The model only sees the merged system text — not these paths.

Slot labels (`[Screen after action]`, …) are defined in `vision/screen_overlay.rs` and injected **immediately before each image** in the `[CUR_SCREEN]` user message (see `extension_hooks/screen_inject.rs`). The planning layout is: optional reused previous capture, current, annotated, and `[Zoom pointer after action]`.
The previous capture is the stored JPEG, not a new screenshot.

## Layout

```
prompts/
  tiers/
    primary/       communication.md + loop.md   ← runtime source
  os/              macos.md | windows.md | linux.md
  ui_disabled_controls.md   shared — gray/disabled controls
  modules/verify/  host post-execute verify LLM prompts (not in main system merge)
author/            not loaded (authoring reference only)
AGENT.md           manifest + config (repo root of `computer/`)
```

## Edit guide

| Change | File |
|--------|------|
| Role / scenes / turn loop / queue board (runtime) | `tiers/primary/communication.md`, `tiers/primary/loop.md` |
| OS-specific hints | `os/*.md` |
| Disabled UI controls | `ui_disabled_controls.md` |
| Host verify LLM (internal) | `modules/verify/*.md` |

Main prompts state scenes and rules.
Tool arguments stay in `tools/prompts/*.md`.
Positioning order: a label on that control,
then coordinates when the full screen shows a visible interior,
then move beside a mark that has no interior to aim at.
The next turn reads `[Zoom pointer after action]`.
Set `zoom: true` and give 0–1000 of that crop.
The host maps those coordinates onto the full screen.
Do not add fill-in templates (`Next`, `Repetition`, `Locate`, ratio math).
