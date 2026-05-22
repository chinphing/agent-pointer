### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at the current cursor, or a small offset move.

**This session uses overlay index methods only.** Target the numbered region on **`[Annotated after action]`**. **Forbidden:** `click_at`, `hover_at`, `drag_from_to_at`, and any `x`/`y` in `tool_args`.

**Call priority:** Prefer **composite_action**, **hotkey**, or **modified_click** when one call achieves the same goal with fewer steps.

**Index methods** (require `goal`, `action`, `index`; optional `anchor`, `dx`, `dy`):
- **`mouse:click_index`**, **`mouse:double_click_index`**, **`mouse:right_click_index`**, **`mouse:hover_index`**
- **`mouse:drag_from_to_index`** — `from_index`, `to_index` on the current annotated frame

**Anchor:** `center` (default), `top-left`, `top-right`, `bottom-left`, `bottom-right`. **`dx`**, **`dy`**: session **0–1000** offsets from that anchor. Paste **`- R: (left, top, right, bottom)`** verbatim (**MA-3 FOUND**); then **W/H**, **`f_x`/`f_y`**, **`dx`/`dy`** per **communication** *Phase C* — **forbidden** inventing the bullet. **Verify:** UI outcome vs **goal** — pointer on target alone is **not** **pass** for click/copy goals.

When the control is **inside** a large bbox, use **inside-R** offsets. When it sits **outside** the bbox border, prefer a **tighter index** if one exists; else **outside-R** math (**f_x/f_y** may be **<0** or **>1**, **|dx|** may exceed **W**) per **communication** *Phase C*.

Optional **`human_like`** (bool) on index clicks and drags.

**Current cursor (no index move):** **`mouse:click_current`**, **`mouse:double_click_current`**, **`mouse:right_click_current`** — only **`goal`**, **`action`**. Use when the cursor is already on the target.

**Other:** **`mouse:scroll_at_current`** (`goal`, `action`, `lines`) — cursor must already be inside the scrollable area.

**Offset move:** **`mouse:move_offset`** (`goal`, `action`, `dx`, `dy`) — nudge from current position.

Parameter constraints:
- **`goal`** and **`action`** are required.
- **`index`** must match a visible overlay digit on the current annotated screenshot.

**Optional `wait` in `tool_args`:** After successful calls (1–5 s). Heuristic: **~1–2** s for simple clicks; **~3–5** s for dialogs.

**Reposition:** **MA-3 NOT FOUND** → **`mouse:hover_index`** on **R** (`center`, `dx=0`, `dy=0`) — no click/type that turn.

**Scroll workflow:** **`mouse:hover_index`** or **`mouse:click_index`** on the scroll region, then **`mouse:scroll_at_current`** if needed.

#### JSON examples — `mouse:click_index`

**A — bbox ≈ control (no offset):**

```json
{
  "thoughts": "…\nNext:\nTarget region: On [Annotated after action]: dialog footer primary button.\nSub-target: button fills bbox 12.\nPick: index 12; anchor center; offset Δx=0, Δy=0\n",
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

**B — compound bbox:** **MA-3 FOUND**, paste inject bullet under **MA-5**, then **W/H**, **`f_x`/`f_y`**, **`dx`/`dy`**.

```json
{
  "thoughts": "…\nNext:\n…\nMA-3 Inject lookup: FOUND\nMA-5 Nearby: - <R>: (<from inject>)\nSub-target: glyph at f_x≈0.85, f_y≈0.5 inside R.\nPick: index <R>; W=… H=…; anchor center; f_x=0.85 f_y=0.5 → (x_sub,y_sub)=(…,…) → dx=… dy=0\n",
  "headline": "Activate control",
  "tool_name": "mouse:click_index",
  "tool_args": {
    "goal": "Activate the option",
    "action": "click trailing control in bbox",
    "index": 14,
    "anchor": "center",
    "dx": 48,
    "dy": 0,
    "wait": "2"
  }
}
```
