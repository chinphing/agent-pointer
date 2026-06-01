### input_index / input_at / input_focused

Flat input tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable description for UI).
Optional: `clear_first`, `auto_enter`, `human_like`, `wait` (1–5 s).

**Index-based** (overlay digit — clicks bbox **center**):
- **`input_index`** — Type text at index. Requires **`index`**, **`text`**.

**Coordinate-based** (session 0–1000):
- **`input_at`** — Type text at coordinates. Requires **`x`**, **`y`**, **`text`**.

**Focused field:**
- **`input_focused`** — Type into focused input. Requires **`text`** only.
