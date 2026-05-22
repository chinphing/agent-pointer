# Computer agent prompts (maintainer index)

Runtime slices are merged in code (`computer_communication_for_tier`, `computer_agent_body_for_tier`).
The model only sees the merged system text — not these paths.

Slot labels (`[Screen after action]`, …) are defined in `vision/screen_overlay.rs` and injected **immediately before each image** in the `[CUR_SCREEN]` user message (see `extension_hooks/screen_inject.rs`).

## Layout

```
prompts/
  tiers/
    primary/       communication.md + loop.md
    intermediate/  communication.md + loop.md
    advanced/      vision_slots.md + communication.md + loop.md
  os/              macos.md | windows.md | linux.md
author/            not loaded (authoring reference only)
AGENT.md           manifest + config (repo root of `computer/`)
```

## Edit guide

| Change | File |
|--------|------|
| Primary — role / 3-step framework / one annotated image | `tiers/primary/communication.md` |
| Primary loop (start checklist) | `tiers/primary/loop.md` |
| Intermediate Part 1–2 (from advanced) + simple Next | `tiers/intermediate/communication.md` |
| Intermediate loop | `tiers/intermediate/loop.md` |
| Advanced **Part 1 Verify** / **Part 2 Repetition** / **Part 3 action** | `tiers/advanced/communication.md` |
| Advanced image order only | `tiers/advanced/vision_slots.md` |
| Advanced loop | `tiers/advanced/loop.md` |
| OS shortcuts | `prompts/os/*.md` |
| Tool JSON / handlers | `tools/` (Rust); tool bodies `tools/prompts/index/*` (Primary+Intermediate) or `tools/prompts/coordinate/*` (Advanced), selected per tier in `tools_system_appendix` |

Full product doc: `docs/agents/computer-agent-prompts.md`.
