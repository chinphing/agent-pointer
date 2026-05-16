## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line).
Older desktop turns are stripped—use **only** this inject.

**Order when a prior turn exists:** **`[Screen before action]`** → **`[Zoom pointer before action]`** → **`[Screen after action]`** → **`[Annotated after action]`** → **`[Zoom top after action]`** → **`[Zoom bottom after action]`** → **`[Zoom pointer after action]`**. First capture in a thread omits the two **before** slots.

### Full-screen

- **`[Screen before action]`** (if present) — **previous** turn’s unmarked capture with the **current** synthetic pointer:
  **pre-action** desktop layout. Name **Intended aim** on this frame in **`Pointer:`** line **1**; hotspot geometry uses **`[Zoom pointer before action]`** (see **Zooms**). **UI change** vs after → **`Verify:`** **`Before vs after`** only, not **`Pointer:`**.

- **`[Screen after action]`** — **after** that step; full-screen layout truth + synthetic pointer/caret.
  In **internal** stages **1–4** (**`Pointer:`** / **`Verify:`** / **`Repetition:`** / **`Next:`**), must **not** name overlay digits,
  **`index`**, or “bbox N”; describe targets from this full-screen frame only.
  **`index`** is allowed **only** in **internal** stage **5** (**`Location:`**).

### Annotated

- **`[Annotated after action]`** — same moment as **`[Screen after action]`**,
  with a **digit per detected region**.
  Use **`index`** in **`mouse` / composite / modified_click`** overlay methods
  only when **one** region’s **bbox** wraps **a single** target (one control / one icon / one field alone).
  If the **bbox** encloses **multiple** distinct elements—label + field, several icons, title + chips in one **bbox**, etc.—treat as **multiple** even for one semantic “row”; use **coordinates** instead, because **`index`** clicks the **region center** and will miss the intended sub-target.
  In **internal** stage **5** (**`Location:`**): **line 1** grounds the target from **`Next:`** **line 2** — **no** overlay **`index`** on **line 1** (**including** **`neighbors:`**; e.g. “**(index 34)**” is forbidden — layout/label/stroke only). **line 2** picks **`index` N** only after **(a)(b)(c)** when digit **background** = **`bbox` border stroke** and the digit is **flush-adjacent only** to that **`bbox`**. **bbox wrap count** (**`inventory:`**) = **only** whether that **`bbox`** wraps **one** vs **multiple** targets; then choose **`index`** or **coordinates**. On **coordinates**, anchor on **`[Zoom pointer after action]`** when the sub-target is in the pointer zoom (else **`[Screen after action]`** or the **line 1** frame); map **(x, y)** via **Pointer position**, not from overlay **`index`** clicks.

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

### Zooms (crops — not full-screen)

**Before-action** (if present; sourced from **`[Screen before action]`**):

- **`[Zoom pointer before action]`** — **4×** magnified **100×100 px** crop (**±50 px** radius around the pointer), with the **synthetic pointer always drawn** on the crop.
  **`Pointer:`** / **`Verify:`** mouse geometry uses this image as the **standard** (hotspot vs intended center). **Do not** use “pointer not visible” as **`n/a`** when this slot is present.

**After-action** (same moment as **`[Screen after action]`** / **`[Annotated after action]`**):

- **`[Zoom top after action]`** — top strip (menu bar / title): small chrome, app name, top-edge controls.
- **`[Zoom bottom after action]`** — bottom strip (dock / taskbar): launcher icons, status UI.
- **`[Zoom pointer after action]`** — **300×300 px** annotated patch around pointer at capture;
  prefer for coordinate **`*_at`** layout and fine detail under the cursor.

In **`Location:`**, **first** state **placement / bearing** on **`[Screen after action]`** (**top / bottom / near pointer / central** from **`Next:`** line 2 band), **then** **`therefore analyze on [Zoom … | Annotated …]`** — **then** **`bbox` → `index`**. Do **not** skip bearing and pick a frame by habit.

| Bearing on full screen | Prefer overlay frame |
|------------------------|----------------------|
| **Top** — menu, title, tabs under chrome | **`[Zoom top after action]`** (crowded digits) or **`[Annotated after action]`** |
| **Bottom** — dock / taskbar | **`[Zoom bottom after action]`** |
| **Near synthetic pointer** — row chip, inline control | **`[Zoom pointer after action]`** |
| **Central / wide** — dialog body, full toolbar | **`[Annotated after action]`** |

On the **chosen** frame, match **`index`** when **digit background** = **`bbox` border** and the digit is **flush-adjacent only** to that **`bbox`**. When you cite a **bbox**, name **which frame** you used.
