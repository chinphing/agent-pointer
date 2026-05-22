# Computer agent (Rust layout)

```
computer/
  AGENT.md              # manifest (YAML config)
  mod.rs                # module tree + public re-exports
  state/                # ComputerState, sessions, capture pipeline
  input/                # mouse/keyboard executor, enigo, timing
  vision/               # screenshot, annotate API, coords, overlays, VisionState
  tier/                 # Primary / Intermediate / Advanced runtime
  verify.rs             # tool result hint strings for the model
  capture_debug.rs      # optional on-disk capture dumps
  extension_hooks/      # LLM prompt inject (CUR_SCREEN, LOCKED GOAL, …)
  tools/                # ToolRegistry handlers + prompts/
  prompts/              # tier communication + loop (see prompts/README.md)
  author/               # not loaded
  assets/
```

Product docs: `docs/agents/computer-agent-prompts.md`.

Public API paths (`crate::agents::computer::screen`, `::actions`, etc.) are preserved via `mod.rs` re-exports.
