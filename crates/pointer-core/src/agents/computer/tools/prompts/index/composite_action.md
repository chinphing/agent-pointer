### composite_action

Use for **one-call combos**: click+type, or focus-then-type.

**This session uses overlay index methods only.** **Forbidden:** `type_text_at` and any bare `x`/`y` in `tool_args`.

**Call priority:** Use **composite_action** when this turn must **type literal `text` into a field**. For click-only steps, use **`mouse:click_index`**.

**Index methods:**
- **`composite_action:type_text_at_index`** (`goal`, `action`, `index`, `text`, optional `clear_first`, optional `auto_enter`, optional `anchor`, `dx`, `dy`) — Clicks the indexed region then types. If the field already has focus, use **`type_text_at_focused`** instead.
- **`composite_action:scroll_at_index`** (`goal`, `action`, `index`, `lines`) — Move to the indexed scroll region, then scroll.

**Focused field (no new click):**
- **`composite_action:type_text_at_focused`** (`goal`, `action`, `text`, optional `clear_first`, optional `auto_enter`) — Types into the focused input.

Parameter constraints:
- **`goal`**, **`action`**, **`text`** required where applicable.
- `auto_enter` defaults to **false**. On **Windows PowerShell / terminal**, set `auto_enter=true` when Enter should run the command.

**Optional `wait` in `tool_args`:** **~2–4** s after type-and-submit; **~1–2** s for focus+type.
