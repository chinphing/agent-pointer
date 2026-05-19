## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line).
Older desktop turns are stripped—use **only** this inject.

**Order when a prior turn exists:** **`[Screen before action]`** → **`[Zoom pointer before action]`** → **`[Screen after action]`** → **`[Annotated after action]`** → **`[Zoom top after action]`** → **`[Zoom bottom after action]`** → **`[Zoom pointer after action]`**. First capture in a thread omits the two **before** slots.

### Full-screen

- **`[Screen before action]`** (if present) — **previous** turn’s unmarked capture with the **current** synthetic pointer:
  **pre-action** desktop layout. Name **Intended aim** on this frame in **`Pointer:`** line **1**; hotspot geometry uses **`[Zoom pointer before action]`** (see **Zooms**). **verify stage** **`Before vs after`** must compare **`[Screen before action]`** → **`[Screen after action]`** (name both frames); not **`Pointer:`**. **`Clear evidence`** restates that **UI** delta only — not click/tool success or pointer accuracy (**`Mouse judgment:`** / **`Pointer:`**).

- **`[Screen after action]`** — **after** that action; full-screen layout truth + synthetic pointer/caret.
  In **internal** stages **1–4** (**`Pointer:`** / **`Verify:`** / **`Repetition:`** / **`Next:`**), must **not** name overlay digits,
  **`index`**, or “bbox N”; describe targets from this full-screen frame only.
  **`Next:`** is **target only** (two numbered lines) — **no** tool choice, **no** overlay **`index`**, **no** coordinates.
  **Overlay analysis** is **internal** stage **5** (**`Location:`**); **tool** routing is **internal** stage **6** (**`Tool route:`** line **2**).

### Annotated

- **`[Annotated after action]`** — same moment as **`[Screen after action]`**,
  with a **digit per detected region**.
  Use **`index`** in **`mouse` / composite / modified_click`** overlay methods
  only when **one** region’s **bbox** wraps **a single** target (one control / one icon / one field alone).
  If the **bbox** encloses **multiple** distinct elements—label + field, several icons, title + chips in one **bbox**, etc.—treat as **multiple** even for one semantic “row”; use **coordinates** instead, because **`index`** clicks the **region center** and will miss the intended sub-target.
  In **internal** stage **5** (**`Location:`**): **line 1** — **`traits inside that bbox:`** lists **`distinct controls`**, **`wrap count`**, **`intended sub-target`** (visual names only; no overlay digits on line 1). **Lines 2–4 follow `traits`:** **line 2** **`index` N** after **(a)(b)(c)**; **line 3** route from **`wrap count`** only (**1** → index, **>1** → coordinates); **line 4** when coordinates — if **`N`** is in **Pointer neighbor reference bboxes**, **(x,y)** on **`[Annotated after action]`**; if **`N`** is **not** listed, **geometry deferred** → **`hover_index`** on **`N`**. **line 6** **`Tool route:`** = explicit tool matching root **`tool_name`**.

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

- **`[Zoom pointer before action]`** — **4×** magnified crop around the pointer (**±100 px** radius on the full screen).
  **`Pointer:`** / **verify stage** mouse geometry uses this image as the **standard** (hotspot vs intended center). **Do not** use “pointer not visible” as **`n/a`** when this slot is present.

**After-action** (same moment as **`[Screen after action]`** / **`[Annotated after action]`**):

- **`[Zoom top after action]`** — top strip (menu bar / title): small chrome, app name, top-edge controls.
- **`[Zoom bottom after action]`** — bottom strip (dock / taskbar): launcher icons, status UI.
- **`[Zoom pointer after action]`** — **4×** magnified annotated crop around the pointer; lines **1–2** when digits are small.

In **`Location:`**, **first** state **placement / bearing** on **`[Screen after action]`** (**top / bottom / near pointer / central** from **`Next:`** line 2 band), **then** **`therefore analyze on [Zoom … | Annotated …]`** — **then** **`bbox` → `index`**. Do **not** skip bearing and pick a frame by habit.

| Bearing on full screen | Prefer overlay frame |
|------------------------|----------------------|
| **Top** — menu, title, tabs under chrome | **`[Zoom top after action]`** (crowded digits) or **`[Annotated after action]`** |
| **Bottom** — dock / taskbar | **`[Zoom bottom after action]`** |
| **Near synthetic pointer** — row chip, inline control | **`[Zoom pointer after action]`** |
| **Central / wide** — dialog body, full toolbar | **`[Annotated after action]`** |

On the **chosen** frame, match **`index`** when **digit background** = **`bbox` border** and the digit is **flush-adjacent only** to that **`bbox`**. When you cite a **bbox**, name **which frame** you used.
