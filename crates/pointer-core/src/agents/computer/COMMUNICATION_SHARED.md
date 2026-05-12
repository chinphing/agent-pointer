## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line). Older desktop turns are stripped—use **only** this inject.

### Full-screen

- **`[Screen before action]`** (if present) — desktop **before** the last automated step; with **`[Screen after action]`** compare windows, focus, typed text, scroll.
- **`[Screen after action]`** — **after** that step; layout truth + synthetic pointer/caret. In `<thoughts>`, **Verify**, **repetition**, and **Next action** must **not** name overlay digits, **`index`**, or “box N”; describe targets from this full-screen frame only. **`index`** is allowed **only** in **Target location** (slim `<thoughts>` template).

### Annotated

- **`[Annotated after action]`** — same moment as **`[Screen after action]`**, with a **digit per detected region**. Use **`index`** in **`mouse` / composite / modified_click`** overlay methods only when **one** region’s box tightly matches the target.
- **Digit ↔ bbox:** Each **digit and its outline** are **one pair**—they use the **same color** for that region; match digit to box by **shared color** as well as placement (the integer sits **on** the region that pair labels, not a neighbor’s outline). **`index`** must be **exactly** that printed integer. On this frame, labels follow a **fixed enumeration** (**1** = first region in that order, **2** = second, …), matching how the overlays were drawn. **Do not** invent or swap numbers by re-sorting boxes yourself (reading order, size, or overlap guesses); if two outlines sit close together, use **color + placement** to pick the digit on the widget you mean.
- **Overlay vs coordinates:** overlay methods use **`index`** to aim at **that** pair’s region center—the digit is **not** pixel **x,y**. Labels **reset every turn**—never reuse an **`index`** from an older screenshot; always read the **current** **`[Annotated after action]`** (or zoom below). If several digits are plausible, prefer the one **on** the intended control; if one box spans many controls or none fits, use **coordinates** per the inject’s scale instead.

### Zooms (after-action crops)

All zooms align with **`[Screen after action]`** / **`[Annotated after action]`**, not the before frame.

- **`[Zoom top after action]`** — top strip (menu bar / title): small chrome, app name, top-edge controls.
- **`[Zoom bottom after action]`** — bottom strip (dock / taskbar): launcher icons, status UI.
- **`[Zoom pointer after action]`** — patch around pointer at capture; prefer for what is under the cursor and fine detail there.

Pick **`index`**: **zoom pointer** first, then **annotated**. When you cite a box, name **which frame** you used.
