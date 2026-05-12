## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line).
Older desktop turns are stripped—use **only** this inject.

### Full-screen

- **`[Screen before action]`** (if present) — desktop **before** the last automated step;
  with **`[Screen after action]`** compare windows, focus, typed text, scroll.

- **`[Screen after action]`** — **after** that step; layout truth + synthetic pointer/caret.
  In **internal** stages **1–4** (**`Pointer:`** / **`Verify:`** / **`Repetition:`** / **`Next:`**), must **not** name overlay digits,
  **`index`**, or “box N”; describe targets from this full-screen frame only.
  **`index`** is allowed **only** in **internal** stage **5** (**`Location:`**).

### Annotated

- **`[Annotated after action]`** — same moment as **`[Screen after action]`**,
  with a **digit per detected region**.
  Use **`index`** in **`mouse` / composite / modified_click`** overlay methods
  only when **one** region’s box wraps **a single** target (one control / one icon / one field alone).
  If the outline encloses **multiple** distinct elements—label + field, several icons, title + chips in one box, etc.—treat as **multiple** even for one semantic “row”; use **coordinates** instead, because **`index`** clicks the **region center** and will miss the intended sub-target.
  In **internal** stage **5** (**`Location:`**): **Re-compare** = **inclusion** only—the intended target from **`Next:`** **inside** this candidate **`index`** bbox or not (re-scan other digits when not). **BBox wrap count** = **only** whether that bbox wraps **one** vs **multiple** targets; then choose **`index`** or **coordinates**. Do not use inclusion to decide single vs multiple, or vice versa.

- **Digit ↔ bbox:** Each **printed index** has a **colored background** behind the glyph; that **background color** matches the **outline color** of the paired region (**not** “digit ink color = outline color”). Match **`index`** to box by **background color + outline color** and placement
  (the integer sits **on** the region that pair labels, not a neighbor’s outline).
  **`index`** must be **exactly** that printed integer.
  On this frame, labels follow a **fixed enumeration** (**1** = first region in that order, **2** = second, …),
  matching how the overlays were drawn.
  **Do not** invent or swap numbers by re-sorting boxes yourself
  (reading order, size, or overlap guesses);
  if two outlines sit close together, use **background color** vs **outline color** and **placement** to pick the digit on the widget you mean.

- **Overlay vs coordinates:** overlay methods use **`index`** to aim at **that** pair’s region center—
  the digit is **not** pixel **x,y**.
  Labels **reset every turn**—never reuse an **`index`** from an older screenshot;
  always read the **current** **`[Annotated after action]`** (or zoom below).
  If several digits are plausible, prefer the one **on** the intended control;
  if one box spans **multiple** controls or none fits, use **coordinates** per the inject’s scale instead.

### Zooms (after-action crops)

All zooms align with **`[Screen after action]`** / **`[Annotated after action]`**,
not the before frame.

- **`[Zoom top after action]`** — top strip (menu bar / title): small chrome, app name, top-edge controls.
- **`[Zoom bottom after action]`** — bottom strip (dock / taskbar): launcher icons, status UI.
- **`[Zoom pointer after action]`** — patch around pointer at capture;
  prefer for what is under the cursor and fine detail there.

Pick **`index`** on **`[Annotated after action]`** or on **any** after-action zoom — **`[Zoom top after action]`**, **`[Zoom bottom after action]`**, **`[Zoom pointer after action]`** — whichever frame shows **background color** behind the index **matching** **outline color** most clearly; all zoom crops are valid references. When you cite a box, name **which frame** you used.
