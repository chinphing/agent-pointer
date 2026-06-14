# Exploration shared prompts (maintainer index)

Runtime slices are merged in `coder/mod.rs` and `explore/mod.rs`. The model only sees merged system text.

| File | Loaded by |
|------|-----------|
| `impact_scan.md` | explore body only |
| `handoff_contract.md` | explore body |
| `trace_when.md` | explore body |
| `file_discipline.md` | explore body; referenced by coder COMMUNICATION |

Dev doc: `docs/agents/coder-explore-prompts.md`.
