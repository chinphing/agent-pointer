## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line).
Older desktop turns are stripped—use **only** this inject.

### Full-screen

- **`[Screen before action]`** (if present) — desktop **before** the last automated step;
  with **`[Screen after action]`** compare windows, focus, typed text, scroll.

- **`[Screen after action]`** — **after** that step; layout truth + synthetic pointer/caret.
  In **internal** stages **1–4** (**`Pointer:`** / **`Verify:`** / **`Repetition:`** / **`Next:`**), must **not** name overlay digits,
  **`index`**, or “bbox N”; describe targets from this full-screen frame only.
  **`index`** is allowed **only** in **internal** stage **5** (**`Location:`**).

### Annotated

- **`[Annotated after action]`** — same moment as **`[Screen after action]`**,
  with a **digit per detected region**.
  Use **`index`** in **`mouse` / composite / modified_click`** overlay methods
  only when **one** region’s **bbox** wraps **a single** target (one control / one icon / one field alone).
  If the **bbox** encloses **multiple** distinct elements—label + field, several icons, title + chips in one **bbox**, etc.—treat as **multiple** even for one semantic “row”; use **coordinates** instead, because **`index`** clicks the **region center** and will miss the intended sub-target.
  In **internal** stage **5** (**`Location:`**): **Re-compare** = **inclusion** only—the intended target from **`Next:`** **line 2** **inside** this candidate **`index`** **bbox** or not (re-scan other digits when not). **bbox wrap count** = **only** whether that **bbox** wraps **one** vs **multiple** targets; then choose **`index`** or **coordinates**. Do not use inclusion to decide single vs multiple, or vice versa.

- **Digit ↔ bbox:** Each **printed index** pairs with **exactly one** **bbox** when **both** hold: **background color** behind the digit **matches** that **bbox**’s **border color** (**not** “digit ink = border color”), **and** the digit sits **tightly on** the **bbox** border—**flush** with the stroke, **not** suspended between two **bbox** regions. Match **`index`** to **bbox** by that **color** tie **plus** **contiguous** placement
  (the integer labels the **bbox** it **touches**, not a neighbor’s **bbox**).
  **`index`** must be **exactly** that printed integer.
  On this frame, labels follow a **fixed enumeration** (**1** = first region in that order, **2** = second, …),
  matching how the overlays were drawn.
  **Do not** invent or swap numbers by re-sorting **bbox** regions yourself
  (reading order, size, or overlap guesses);
  if two **bbox** regions sit close together, use **background color** vs **bbox** border color and **placement** to pick the digit on the widget you mean.

- **Overlay vs coordinates:** overlay methods use **`index`** to aim at **that** pair’s region center—
  the digit is **not** pixel **x,y**.
  Labels **reset every turn**—never reuse an **`index`** from an older screenshot;
  always read the **current** **`[Annotated after action]`** (or zoom below).
  If several digits are plausible, prefer the one **on** the intended control;
  if one **bbox** spans **multiple** controls or none fits, use **coordinates** per the inject’s scale instead.

### Zooms (after-action crops)

All zooms align with **`[Screen after action]`** / **`[Annotated after action]`**,
not the before frame.

- **`[Zoom top after action]`** — top strip (menu bar / title): small chrome, app name, top-edge controls.
- **`[Zoom bottom after action]`** — bottom strip (dock / taskbar): launcher icons, status UI.
- **`[Zoom pointer after action]`** — patch around pointer at capture;
  prefer for what is under the cursor and fine detail there.

Pick **`index`** on **`[Annotated after action]`** or on **any** after-action zoom — **`[Zoom top after action]`**, **`[Zoom bottom after action]`**, **`[Zoom pointer after action]`** — whichever frame shows **background color** behind the index **matching** **bbox** border color **and** the digit **tightly on** that **bbox** border most clearly; all zoom crops are valid references. When you cite a **bbox**, name **which frame** you used.
