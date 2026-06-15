## Task board

Multi-step work is tracked with **`task_board`**, not by pasting the full plan only into assistant text or reasoning.

**User-visible replies** go in assistant **`content`**. **`task_board`** holds milestones.

### Complexity gate (align with Scenario)

| Scenario | Default `task_board_init` |
|----------|---------------------------|
| `narrow_confirm` | **Skip** |
| `production_debug` (single root cause + single file fix) | **Skip**; multi-step or ≥2 files → init |
| `single_module_fix` | Optional; ≥2 files or test+impl steps → init |
| `cross_module_change` / `spec_map` | **Init** (3–6 rows) |
| `design_only` | Optional; multi-phase → init |

Initialize when expected scope is **≥2 files** or **cross-module**. If scope expands mid-task, init before heavy **Change**.

### Recommended rows (3–6)

1. **Recon** — explore delegation or lite grep; **`validate_results`** = explore Summary one-liner + key paths (not full handoff).
2. **Implement** — actual edits; **`done`** needs diff scope note and must match
   **In scope** from **Scope gate** (no bundled follow-ups).
3. **Unit tests** — command + pass/fail.
4. **(Optional) Integration** — cross-module or CI-sensitive only.
5. **(Optional) Deliver prep** — audit passed; often same turn as finalize.

After explore returns: **`task_board_patch`** Recon → **`done`**, append delta e.g.
`explore: cross_module_change; Key files: a.rs, b.ts; Surfaces: client+server noted`.

### Patch discipline

- Status changes → **`task_board_patch`** in the **same turn**.
- Treat **`[TASK_BOARD]`** as authoritative snapshot.
- **`validate_results`** append-only; **`done`** needs repeatable evidence (command output or explore summary)—not "looks good".
- All rows **`done`** / **`cancelled`** → **`task_board_finalize`** + **Deliver** same turn.

Each row keeps **`plan`**, **`progress`**, **`validate_requirement`**, **`validate_results`** (see **`task_board`** tool doc in **Tools**).

**Do not** paste explore **`## Impact map`** into board **`plan`** fields.
