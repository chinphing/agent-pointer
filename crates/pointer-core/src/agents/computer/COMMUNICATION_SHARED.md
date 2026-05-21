## Desktop vision (`[CUR_SCREEN]`)

Ordered images under **`[CUR_SCREEN]`** (slot names in the first line).
Older desktop turns are stripped — use **only** the current **`[CUR_SCREEN]`** block.

**Order when a prior turn exists:** **`[Screen before action]`** → **`[Zoom pointer before action]`** → **`[Screen after action]`** → **`[Annotated after action]`** → **`[Zoom top after action]`** → **`[Zoom bottom after action]`** → **`[Zoom pointer after action]`**. First capture omits the two **before** slots.

---

### Frame registry (which stage reads which slot)

| Slot | Content | **Must read in** | **Used for** |
|------|---------|------------------|--------------|
| **`[Screen before action]`** | Prior turn unmarked full-screen + **current** synthetic pointer | **Pointer** L1; **Verify** Before vs after (before) | Pre-action layout; name intended aim |
| **`[Zoom pointer before action]`** | **4×** crop ±50 px from before frame, centered on pointer | **Pointer** L2; **Verify** Mouse judgment cite | Hotspot vs center — **required** when slot present |
| **`[Screen after action]`** | Current full-screen + synthetic pointer | **Next** L2; **Location** L1 placement; **Verify** Before vs after (after) | Current layout truth |
| **`[Annotated after action]`** | Same moment as after screen + overlay digits per region | **Location** L2–L3; coordinate **(x,y)** on L3 | Bbox strokes, digit↔bbox pairing |
| **`[Zoom top after action]`** | Top strip crop (menu / title) | **Location** overlay frame pick | Top-band targets |
| **`[Zoom bottom after action]`** | Bottom strip crop (dock / taskbar) | **Location** overlay frame pick | Bottom-band targets |
| **`[Zoom pointer after action]`** | **4×** annotated crop around pointer | **Location** overlay frame pick | Near-pointer targets; small digits |

**Also included (text, not an image slot):** **Pointer position** + **Overlay reference bboxes** — **every** overlay index with session-scale corner/center coordinates for **Location** line **3** geometry. Indices are **anchors only** — final tools use **(x,y)**.

**Rule:** Every visual claim in **`thoughts`** must cite **`On [slot name]:`**. **Forbidden** to describe UI from task text without reading the slot.

---

### Digit ↔ bbox pairing (on annotated / zoom annotated frames)

Each printed **index** pairs with **exactly one bbox** when **both** hold:

1. **Background color** behind the digit **matches** that bbox's **border color**.
2. The digit sits **tightly on** the bbox border — **flush** with the stroke, **not** between two bbox regions.

Labels follow **fixed enumeration** on the current frame (**1** = first region, **2** = second, …). **Do not** re-sort or invent numbers.

**Overlay vs coordinates:** Digits label bboxes — use them as **reference index R** only. Lookup corner/center in **Overlay reference bboxes** row **R**; compute **(x,y)**; call **`click_at`** / **`type_text_at`**. Labels **reset every turn**.

---

### Location overlay frame pick (after reading `[Screen after action]`)

| Bearing on **`[Screen after action]`** | Analyze on |
|----------------------------------------|------------|
| **Top** — menu, title, tabs | **`[Zoom top after action]`** if digits crowded; else **`[Annotated after action]`** |
| **Bottom** — dock, taskbar | **`[Zoom bottom after action]`** |
| **Near synthetic pointer** | **`[Zoom pointer after action]`** |
| **Central / wide** — dialog, toolbar | **`[Annotated after action]`** |

**Location line 2 (three steps):** digit↔bbox pairing + bbox **W×H** → **band|text|fill|size** facts → relative position vs landmark (words only). **Recheck R2** uses the same four fields. Follow **B2 Visual facts** in communication rules.

**Location line 3:** anchor (5) + direction (8) or **`on anchor`** → copy integers from **Overlay reference bboxes** row **R** → offset → arithmetic → **`therefore (x,y) ≈ (X, Y)`** (non-negative integers).

---

### Stages 1–4 vs 5–7

**execute only** — run on **execute** intent (**intent turn**) and on every **continuation turn**. Skip only on **intent turn** + **analyze** / **plan** / **clarify** (see **Turn kind** / **User intent** in communication rules).

- **Stages 1–4** (**Pointer**, **Verify**, **Repetition**, **Next**): describe targets from **`[Screen before/after action]`** only — **no** overlay digits.
- **Verify V1**: coordinate rows end with **`; pointer at (x,y)=(…)`** (synthetic pointer position — **not** **`executed`**). **Before vs after** opens **`Compare differences from visual information only — no speculation.`**
- **Stage 5** (**Location**): line **2** three-step layout proof, then integer **(x,y)** on line **3** (anchor/direction + **Overlay reference bboxes** row **R**).
- **Stage 6** (**Recheck coordinates**): only when stage **5** has **(x,y)** — **precision_miss** 3px loop guard + target-at-**(X,Y)** vs **Next** line **2**; revise **R** / **(x,y)** if needed.
- **Stage 7** (**Tool route**): **`click_at`** / **`type_text_at`** / **`modified_click_at`** at final integer **(x,y)**; **forbidden** all **`*_index`** methods and **`index:`** args **every turn**.
