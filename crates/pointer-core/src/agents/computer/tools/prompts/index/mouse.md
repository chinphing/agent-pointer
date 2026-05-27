### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

Use overlay indices as the default route in this guide. Target the numbered region on **`[Annotated after action]`**.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps.

**Index methods** (require `goal`, `action`, `index`):
- **`mouse:click_index`**, **`mouse:double_click_index`**, **`mouse:right_click_index`**, **`mouse:hover_index`**
- **`mouse:drag_from_to_index`** — `from_index`, `to_index` on the current annotated frame

Use the chosen overlay region center directly. Paste **`- R: (left, top, right, bottom)`** verbatim when needed (**MA-3 FOUND**). **Verify:** UI outcome vs **goal** — pointer on target alone is **not** **pass** for click/copy goals.

When the control is **inside** a large bbox, prefer a tighter dedicated index. When it sits **outside** the bbox border, switch to a tighter index or use coordinate tools instead of index-offset math.

Optional **`human_like`** (bool) on index clicks and drags.

**Current cursor (no index move):** **`mouse:click_current`**, **`mouse:double_click_current`**, **`mouse:right_click_current`** — only **`goal`**, **`action`**. Use when the cursor is already on the target.

**Other:** **`mouse:scroll_at_current`** (`goal`, `action`, `lines`) — cursor must already be inside the scrollable area.

**Offset move:** **`mouse:move_offset`** (`goal`, `action`, `dx`, `dy`) — nudge from current position.

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`index`** must match a visible overlay digit on the current annotated screenshot.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic: **~1–2** s for simple clicks; **~3–5** s for dialogs.

**Reposition:** **MA-3 NOT FOUND** → **`mouse:hover_index`** on **R** center — no click/type that turn.

**Scroll workflow:** **`mouse:hover_index`** or **`mouse:click_index`** on the scroll region, then **`mouse:scroll_at_current`** if needed.

#### JSON examples — `mouse:click_index`

**A — bbox ≈ control (no offset):**

```json
{
  "thoughts": "… Route: index — On [Annotated after action]: bbox 12 inner-center-wrap at center → click_index 12.",
  "headline": "Confirm",
  "tool_name": "mouse:click_index",
  "tool_args": {
    "goal": "Confirm the dialog",
    "action": "click primary confirm button",
    "index": 12,
    "wait": "2"
  }
}
```

**B — compound area:** choose a tighter dedicated index first; avoid offset fields.

```json
{
  "thoughts": "… Route: index — On [Annotated after action]: bbox 27 inner-center-wrap; trailing control at center → click_index 27.",
  "headline": "Activate control",
  "tool_name": "mouse:click_index",
  "tool_args": {
    "goal": "Activate the option",
    "action": "click trailing control in bbox",
    "index": 27,
    "wait": "2"
  }
}
```
