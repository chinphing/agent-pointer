### composite_action_type_text_index / composite_action_scroll_index / composite_action_type_text_at / composite_action_type_text_focused

Flat composite tools — each tool name is the complete operation. No `method` parameter.

Every call requires **`goal`** (outcome) and **`action`** (human-readable description for UI).
Optional: `clear_first`, `auto_enter`, `human_like`, `wait` (1–5 s).

**Index-based** (overlay digit — clicks bbox **center**):
- **`composite_action_type_text_index`** — Type text at index. Requires **`index`**, **`text`**.
- **`composite_action_scroll_index`** — Scroll at index. Requires **`index`**, **`lines`**.

**Coordinate-based** (session 0–1000):
- **`composite_action_type_text_at`** — Type text at coordinates. Requires **`x`**, **`y`**, **`text`**.

**Focused field:**
- **`composite_action_type_text_focused`** — Type into focused input. Requires **`text`** only.
