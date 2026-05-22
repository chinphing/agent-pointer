### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

**This session uses overlay index methods only.** Target the numbered region on **`[Annotated after action]`**. **Forbidden:** `click_at`, `hover_at`, `drag_from_to_at`, and any `x`/`y` in `tool_args`.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps.

**Index methods** (require `goal`, `action`, `index`; optional `anchor`, `dx`, `dy`):
- **`mouse:click_index`**, **`mouse:double_click_index`**, **`mouse:right_click_index`**, **`mouse:hover_index`**
- **`mouse:drag_from_to_index`** — `from_index`, `to_index` on the current annotated frame

**Anchor:** `center` (default), `top-left`, `top-right`, `bottom-left`, `bottom-right`. **`dx`**, **`dy`**: optional session 0–1000 offsets from that anchor.

Optional **`human_like`** (bool) on index clicks and drags.

**Current cursor (no index move):** **`mouse:click_current`**, **`mouse:double_click_current`**, **`mouse:right_click_current`** — only **`goal`**, **`action`**. Use when the cursor is already on the target.

**Other:** **`mouse:scroll_at_current`** (`goal`, `action`, `lines`) — cursor must already be inside the scrollable area.

**Offset move:** **`mouse:move_offset`** (`goal`, `action`, `dx`, `dy`) — nudge from current position.

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`index`** must match a visible overlay digit on the current annotated screenshot.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic: **~1–2** s for simple clicks; **~3–5** s for dialogs.

**Scroll workflow:** **`mouse:hover_index`** or **`mouse:click_index`** on the scroll region, then **`mouse:scroll_at_current`** if needed.

#### JSON example — `mouse:click_index`

```json
{
  "thoughts": "Verify:\nOn [Annotated after action]: blue Save pill labeled 12 at dialog footer.\n…\nNext: click Save.\n",
  "headline": "Click Save",
  "tool_name": "mouse:click_index",
  "tool_args": {
    "goal": "Activate Save in the dialog",
    "action": "click the blue Save control",
    "index": 12,
    "wait": "2"
  }
}
```
