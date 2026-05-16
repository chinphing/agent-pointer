## On-wire shape: JSON object

Each desktop reply is **one** JSON object: string fields **`thoughts`**, **`headline`**, optional **`sidecar_tools`** array, then root **`tool_name`** and object **`tool_args`** (schema per tool prompt).

**`thoughts`** — Holds the **five-stage block** below (**`Pointer:`** through optional **`Location:`**), in order, using the **English prefix lines** only for that reasoning. Stages **1–4**: **no** overlay **`index`** / “bbox N”; inside **`Location:`**, **`index`** appears **only** at the **end** of **line 2** (after **(a)(b)(c)**) and on **line 4** **`Outcome`** when the **index** route wins — **never** on **line 1** (including **`neighbors:`**), and **never** lead any line with the chosen **`index`**.

Complete examples at the end of this document use **full JSON**. Less important fields use **`...`**.

## Reasoning framework (every tool or final turn)

Run **five** stages **in order**. Use **exactly** these **English prefix lines**:

- **`Pointer:`** — stage 1 (**numbered lines `1`–`2` + `3 Conclusion (Center-only rule)`** — geometry only; **no** before/after UI delta)
- **`Verify:`** — stage 2  
- **`Repetition:`** — stage 3  
- **`Next:`** — stage 4 (**numbered lines `1`–`2`** + **`Tool kind`**)  
- **`Location:`** — stage 5 **only** when this turn’s method picks a **new** overlay **`index`** or screenshot **`x`/`y`** (or drag endpoints) from the current injects. **Omit** the whole **`Location:`** block for **`wait`**, **`clipboard`**, **`response`**, **`hotkey`**, **`mouse:…_current`**, **`move_offset`**, **`composite_action:type_text_at_focused`**, and any method that does **not** require those targets on the frame.

Within **each** stage, follow that stage’s **Required form** **top to bottom**; **do not** print **`Pointer:`** **`3 Conclusion (Center-only rule)`** / **`Location:`** **line 4** / other **Conclusion** / **Outcome** lines **before** the numbered lines that **earn** them (**Stepwise derivation** in **Ground rules**).

### Ground rules

**No speculation** — Evidence only from current **`[CUR_SCREEN]`** injects, **`[Recent desktop tool calls]`**, and **tool results already in this thread**.
No success from memory or “usually…”.
No clipboard claims without **`clipboard:read`** (or on-screen text).
**Pointer hotspot** for precision clicks: when **`[Zoom pointer before action]`** exists, judge hotspot vs intended center **on that image**
(**4×** magnified **±50 px** crop from **`[Screen before action]`**) — **not** from intent.
**`Pointer:`** does **not** compare before/after UI change (**`Verify:`** **`Before vs after`** only).
**`Pointer:`** does **not** judge **caret** / insertion bar.

**Last automated action — must be grounded (anti-fabrication)** —
**`Pointer:`** **line 1** (Intended aim) and **`Verify:`** judge **only** the **latest** row in **`[Recent desktop tool calls]`** when present (**last line = newest**).
**Do not** invent prior clicks, hotkeys, scrolls, or copy/paste that are **not** on that list.
If the block is **missing** or **empty** → **`none — no prior desktop tool in this thread`**;
**do not** infer a prior action from the user goal.
Default path: **visible UI** actions (**mouse** / **coordinates** / **composite_action** / **hotkey**).
Off-frame inspect tools — **§ Off-frame tools (rare)**.

**Image-grounded clauses** —
**`Pointer:`** **line 1** names the aim frame (**`On [Screen before action]`** / **`[Screen after action]`** in the **Intended aim** prefix);
**line 2** cites **`On [Zoom pointer before action]:`** (or after-action geometry frames when no before zoom).
**`Next:`** **line 2** and **`Location:`** **lines 1–2** must be **prefixed** to a **bracketed inject**.
**Do not** imply pixels without naming the **frame**.

**Full completion** —
**`Step result: pass`** means the **last automated action** succeeded on evidence (including partial batch UI, e.g. **3/10** exported).
**Do not** treat **subset** of the **overall user task** or **repeat “done”** without **new** proof as finished in **`response`**;
**`response`** must match **whole-task** scope, not only **`pass`** on one action.

**Overlay discipline** — Stages **1–4**: **no** overlay **`index`**, digits, or “bbox N”.
**`Location:`** **line 1** (**target on overlay**): **no** overlay numerals anywhere —
**not** in traits, **`neighbors:`**, **`wrapping bbox`**, or **`inventory:`** (neighbor “**(index 34)**” = premature conclusion).
**`index`** **only** at the **end** of **`Location:`** **line 2** and on **line 4** when the **index** route wins.

**Stepwise derivation (mandatory)** — Write **`thoughts`** like a **graded proof**:
**each** stage (**`Pointer:`** … **`Location:`**) and **each** numbered line inside **`Pointer:`** or **`Location:`**
may use **only** facts and conclusions **already shown earlier in that stage** (or in **prior** stages).
**Do not** jump to a final verdict, tool choice, **`index`**, **`x`/`y`**, or **`hover`** **before** the line or stage that **earns** it.
**Do not** skip intermediate substeps or collapse several stages into one sentence
(e.g. no “**`Pointer:`** lines **`1`–`2`** plus **`3 Conclusion (Center-only rule)`** in one line” in real replies).
If a stage does **not** apply (e.g. **`Location:`** omitted), **do not** pretend it ran.

### Tool geometry: overlay **index** vs **coordinates** (computer profile)

Use **`[Annotated after action]`** overlay numbers **only** with **index-based** methods below.
Use **`x`/`y`** (or drag endpoints) with **coordinate-based** methods.
**`[CUR_SCREEN]`** injects **Pointer position** plus **Pointer coordinate anchor** —
use **`[Zoom pointer after action]`** (**300×300 px** crop centered on the pointer) as the **visual anchor** for coordinate calls:
read sub-target layout **on that zoom**, map to session **(x, y)** via **Pointer position** (same space as `*_at` tools).
**`Location:`** **line 4** on the **coordinate path** must cite **`[Zoom pointer after action]`** when the sub-target is visible there;
if **not** in the pointer zoom, fall back to **`[Screen after action]`** or the **line 1** overlay frame — **never** invent coordinates.

**Overlay-index methods** (require an overlay **`index`** / **`indices`** from the current annotated frame):  
**`mouse`:** `mouse:click_index`, `mouse:double_click_index`, `mouse:right_click_index`, `mouse:hover_index`, `mouse:drag_from_to_index` · **`composite_action`:** `composite_action:type_text_at_index`, `composite_action:scroll_at_index` · **`modified_click`:** `modified_click:modified_click_index`.

**Coordinate methods** (require **`x`/`y`** or **`x1`/`y1`/`x2`/`y2`** in the same numeric space as this session’s mouse tool; often **0–1000** normalized on the full capture):  
**`mouse`:** `mouse:click_at`, `mouse:double_click_at`, `mouse:right_click_at`, `mouse:hover_at`, `mouse:drag_from_to_at` · **`composite_action`:** `composite_action:type_text_at` · **`modified_click`:** `modified_click:modified_click_at`.

**Neither index nor typed point on the screenshot:** `mouse:click_current`, `mouse:double_click_current`, `mouse:right_click_current`, `mouse:scroll_at_current`, `mouse:move_offset`, `composite_action:type_text_at_focused`, **`hotkey`**, **`wait`**, **`clipboard:read`**, **`clipboard:write`**, **`response`**.

### Post-action `wait` in `tool_args`

For **`mouse`**, **`hotkey`**, **`composite_action`**, and **`modified_click`**, you may add optional **`wait`** inside **`tool_args`** (seconds, number or numeric string). After a **successful** call, the host waits that long **before** the next **`[CUR_SCREEN]`** screenshot round so the OS/UI can repaint.

- **Clamp:** the runtime enforces **1–5 seconds** (inclusive).
- **Default:** omit **`wait`** — the host still waits a **fixed built-in interval** (about one second) before the next **`[CUR_SCREEN]`** round; use an explicit **`wait`** only when you need **1–5** seconds tuned to UI weight.
- **Choosing a value:** longer for slow surfaces (dialogs opening, navigation, large lists, paste-heavy shortcuts); shorter for light clicks or hovers. Match the weight of the action you just took.
- **Not the `wait` tool:** the standalone **`wait`** tool (`seconds`, blocking pause) is separate—do not confuse it with this **`tool_args`** field.

### Off-frame tools (rare — not the default path)

Most turns use **mouse** / **coordinates** / **composite_action** / **hotkey** on **visible** controls. **`wait`**, **`response`**, and **`clipboard:*`** are **exceptions** — pick them only when the stage chain already earned them; **do not** treat any one exception as the default follow-up.

- **`wait`** (standalone tool) — after **`Step result: pending`** on a **deferred** action when the UI may still be repainting (spinner, dialog transition, queue row appearing). **Omit** **`Location:`**.
- **`response`** — only when **`Verify:`** **`Step result: pass`** on the **last automated action** **and** the **overall** user scope is complete (see **Full completion**); **forbidden** on first turn or while **`pending`** on the active sub-goal.
- **`clipboard:read`** / **`clipboard:write`** — see the **clipboard** tool prompt only; **never** from task narrative alone. **`clipboard:read`** requires a **documented** copy-class row on **`[Recent desktop tool calls]`** plus **`pending`** on that copy action. **Claims** about clipboard text require **`clipboard:read`** result or on-screen text.
- **Pointer `n/a` chain** — any turn **without** pointer geometry (**`wait`**, **`hotkey`**, **`response`**, **`clipboard:read`**, …) — see **§1** mini **tool has no pointer geometry** (**`wait`** example).

---

### 1) Pointer

**1. Goal** — Judge **geometry only** for the **synthetic mouse pointer hotspot** — **not** **caret** (**caret is out of scope**).
**Do not** analyze before/after UI change here (**`Verify:`** only).
**`[Zoom pointer before action]`** (when present) is the **standard** for hotspot vs intended center:
**4×** magnified **100×100 px** crop (**±50 px** radius) from **`[Screen before action]`**.
**`[Screen before action]`** = full layout for **line 1** aim naming.
When **no** before inject, use **`[Screen after action]`** / **`[Zoom pointer after action]`** for geometry.
**`accurate`** = center coincidence on the **geometry image** (**Center-only rule**).
**No** overlay **`index`** in **`Pointer:`**.

**2. Logic** — **(a)** **Intended aim** for the **newest** **`[Recent desktop tool calls]`** row (same action **`Verify:`** judges), **(b)** **`On [Zoom pointer before action]:`** when present — **required** hotspot-vs-center facts; else **`On [Screen after action]:`** / **`[Zoom pointer after action]`**, **(c)** **`3 Conclusion`**. **`n/a`** only for **non-pointer** actions or **no** prior action — when **`[Zoom pointer before action]`** exists, the runtime **always** draws the **synthetic pointer** on that crop (no “invisible pointer” branch).

**3. Template (`Pointer:` chain + analysis flow)**

**`Pointer:` chain (lines `1`–`2` + `3 Conclusion (Center-only rule)`)** — Same **numbered-line discipline** as **`Next:`** and **`Location:`**.

**Pointer chain**

1. **Intended aim** — **Same action as `Verify:` `Last automated action:`** — the **newest** row on **`[Recent desktop tool calls]`** when present
   (**do not** name a control from the user goal or an older row).
   **`Intended aim on [Screen before action]:`** when that inject exists; else **`Intended aim on [Screen after action]:`**.
   For a **precision click** on that row (**`mouse`/`composite_action`/`modified_click`** **index** or **`*_at`**):
   name the control/region **that action tried to hit** on **that** frame — band, label/shape, row — **traits only**; end with **aim = … center**.
   For **`hotkey`**, **`wait`**, **`scroll`**, **`clipboard:*`**, etc. — **`n/a`** (see **Pointer `n/a` chain**).
   **No** verdict words; **no** digits; **no** before/after delta.

2. **Evidence (hotspot vs aim)** — When **`[Zoom pointer before action]`** exists: **`On [Zoom pointer before action]:`** is **required** and is the **standard** for center coincidence (hotspot vs **line 1** center). When **no** before zoom: judge on **`[Screen after action]`** / **`[Zoom pointer after action]`**. **Facts only** — **no** verdict; **no** caret; **no** UI-change narrative.

3. **Conclusion (Center-only rule)** — **`accurate`** \| **`abnormal`** \| **`n/a`** — **one** label, then **reason** that **only** restates **lines 1–2** (no new facts). The reason must **repeat** the **same** hotspot↔target **spatial layout** already given in **line 2**, using **control / region names** — **do not** substitute **`line 1` / `line 2`** for that description. **Do not** use a vague paraphrase alone (e.g. only “misplaced”). **On-wire prefix** must be the literal **`3 Conclusion (Center-only rule):`**.

**Strict derivation inside `Pointer:`** — **`3 Conclusion (Center-only rule)`** is **forbidden** until **lines 1–2** are written **in numeric order**. **Line 1** must **not** embed verdict labels; **line 1** must state the **center** aim when judging a control. **Line 2** holds **all** geometry facts referenced in **`3 Conclusion (Center-only rule)`**, with **frame tags** per **Image-grounded clauses**; **line 2** must locate the hotspot **relative to that center** for **Center-only rule**. **`3 Conclusion (Center-only rule)`** reasons must **name** the **same** widgets/regions as **line 2**.

**Center-only rule (for `accurate` vs `abnormal`)** — Judge on **`[Zoom pointer before action]`** when present (else the after-action geometry frame). **`line 1`** names the control and **aim = its geometric center**. **`accurate`** — hotspot **coincides** with that center on the **geometry image** (allow **only** minimal cursor-art ambiguity — **not** rim / padding). **`abnormal`** — rim, adjacent-only, wrong sub-part, outside silhouette. **`n/a`** — **non-mouse** action only (not “pointer missing” on inject).

**Conclusion labels** — **`accurate`** / **`abnormal`** from **Center-only rule** on the **geometry frame** (**`[Zoom pointer before action]`** when present, else after-action zoom/full-screen). **`n/a`** — **non-mouse** action only.

**Required form**

```text
Pointer:
1 Intended aim on [Screen before action | Screen after action]:
   <must match newest [Recent desktop tool calls] row — same action as Verify Last automated action;
    precision click → traits + aim = center; non-pointer action → n/a;
    no verdict words; no UI delta>.
2 Evidence (hotspot vs aim):
   <On [Zoom pointer before action]: … when present — required standard for geometry>;
    <On [Screen after action] / [Zoom pointer after action]: … only when no before zoom>
   — pointer hotspot only; no caret.
3 Conclusion (Center-only rule):
   <accurate | abnormal | n/a>
   — <reason: name controls/regions + hotspot↔aim spatial relation from Evidence; no new facts>.
```

**Rules (short):** **1** = intended aim for **newest** tool row only (aligned with **`Verify:`**). **2** = **`On [Zoom pointer before action]:`** when present — **standard** for hotspot vs center. **`3`** from **line 2** only.

**4. Mini examples**

**Progressive blocks** (line **1** only, **1–2**) each add **one** new numbered line for teaching; they are **not** complete **`Pointer:`** replies. **Full chains** always emit **`1` → `2` → `3 Conclusion (Center-only rule)`** in that order in real **`thoughts`** — **never** skip **2**, and **never** emit **`3 Conclusion (Center-only rule)`** before **2** (see **anti-patterns**).

**Mini example — line 1 only (Intended aim)**

```text
Pointer:
1 Intended aim on [Screen before action]: Bluetooth toggle knob on second settings row; aim = knob center.
```

**Mini example — lines 1–2 (add evidence; no verdict yet)**

```text
Pointer:
1 Intended aim on [Screen before action]: blue “15” day cell in month grid; aim = cell center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: pointer glyph on **weekday header “Mon”** **above** the blue day cell, **not** over **day-cell center** — offset **north** of **intended day-cell geometric center**.
```

**Mini example — anti-patterns (`Pointer:` numeric order and **`3 Conclusion (Center-only rule)`** wording)**

**Forbidden — numeric order:** **do not** print **`3 Conclusion (Center-only rule)`** before **`2 Evidence`**.

```text
Pointer:
1 Intended aim on [Screen before action]: Bluetooth toggle knob on second settings row; aim = knob center.
3 Conclusion (Center-only rule): abnormal — (forbidden when **Evidence** is missing above).
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: …
```

**Forbidden — vague `3 Conclusion (Center-only rule)`:** it must **restate** the **same** hotspot↔aim **spatial relation** already shown in **line 2** (side, edge, gap, inside/outside bbox, vs center). A bare label like “misplaced” **without** that geometry is **invalid**.

```text
Pointer:
1 Intended aim on [Screen before action]: Bluetooth toggle knob on second settings row; aim = knob center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: synthetic pointer glyph on second row **label text**, **left** of the toggle knob housing, **not** over knob disk center — **lateral left** of knob disk vs **intended knob-disk center**.
3 Conclusion (Center-only rule): abnormal — pointer hotspot misplaced.
```

**Correct — lines `1`→`2`→`3`; concrete `3 Conclusion (Center-only rule)` tied to Evidence:**

```text
Pointer:
1 Intended aim on [Screen before action]: Bluetooth toggle knob on second settings row; aim = knob center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: synthetic pointer glyph on second row **label text**, **left** of the toggle knob housing, **not** over knob disk center — visible **separation** from **intended knob-disk center**.
3 Conclusion (Center-only rule): abnormal — hotspot on **row label left of knob housing**, **lateral gap** to **Bluetooth knob-disk center** — **wrong sub-part** / **not center coincidence**.
```

**Mini example — full chain (`accurate`)**

```text
Pointer:
1 Intended aim on [Screen before action]: blue Save pill in dialog footer; aim = pill center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: synthetic pointer rests on blue Save pill in footer with tip over **pill interior**; hotspot overlaps Save pill **geometric center** vs **intended Save pill center** — **not** on edge band.
3 Conclusion (Center-only rule): accurate — synthetic pointer overlaps **Save pill geometric center** on **[Zoom pointer before action]**.
```

**Mini example — full chain (`abnormal`)**

```text
Pointer:
1 Intended aim on [Screen before action]: triangular Play button in transport strip; aim = button center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: pointer glyph on **progress bar track** **left** of the Play triangle, **not** over **Play-button center** — **lateral left** of **intended Play geometric center**.
3 Conclusion (Center-only rule): abnormal — hotspot on **progress bar track**, not **Play-button geometric center** — **wrong sub-part**.
```

**Mini example — full chain (`abnormal`, inside bbox but on bottom rim — not center)**

```text
Pointer:
1 Intended aim on [Screen before action]: blue Delete pill in footer; aim = pill geometric center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: synthetic pointer on blue Delete pill **flush on bottom rim**, not the middle — lower **edge** of pill footprint vs **intended Delete pill geometric center**.
3 Conclusion (Center-only rule): abnormal — hotspot on **Delete pill bottom rim**, not **pill geometric center**.
```

**Mini example — full chain (`abnormal`, outside bbox — nearby only)**

```text
Pointer:
1 Intended aim on [Screen before action]: star bookmark icon in omnibox strip; aim = icon center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: pointer hotspot in **empty padding left** of the star icon, **outside** the icon’s circular bbox — gap to **star icon center** vs **intended bookmark center**.
3 Conclusion (Center-only rule): abnormal — hotspot in **padding left of star disk**, **outside star silhouette**, not **star / bookmark center**.
```

**Mini example — full chain (no before inject, `abnormal` off-center)**

```text
Pointer:
1 Intended aim on [Screen after action] — first [CUR_SCREEN] in thread; [Screen before action] and [Zoom pointer before action] absent; newest [Recent desktop tool calls] row precision-clicked omnibox URL field; aim = field horizontal center.
2 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer on URL bar **left** edge (off center). On [Zoom pointer after action]: same left-edge placement, **left** of **intended omnibox URL field horizontal-center**.
3 Conclusion (Center-only rule): abnormal — pointer on **URL bar left edge**, not on **URL field horizontal-center**.
```

**Mini example — full chain (`abnormal` — geometry on before zoom; post-action modal irrelevant to Pointer)**

```text
Pointer:
1 Intended aim on [Screen before action]: trash icon on list row; aim = icon center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: pointer hotspot on **row label text**, **left** of trash icon disk, **not** over **trash icon center**.
3 Conclusion (Center-only rule): abnormal — hotspot on **row label**, not **trash icon center** on **[Zoom pointer before action]**.
```

**Mini example — full chain (`n/a`, tool has no pointer geometry)**

```text
Pointer:
1 Intended aim on [Screen after action]: n/a — wait does not target a control center.
2 Evidence (hotspot vs aim): On [Screen after action]: n/a — no pointer hotspot vs click center for this tool class.
3 Conclusion (Center-only rule): n/a — Pointer not used for wait-only geometry.
```

---

### 2) Verify

**Purpose:** Judge the **last automated action** using **Pointer** + before/after screens + grounded tool text —
**only** if that action appears on **`[Recent desktop tool calls]`** (latest row).
**Never** verify an action you only assume from the task narrative.

**Last automated action (required grounding)** — First line: **`Last automated action:`** —
**quote** the **newest** **`[Recent desktop tool calls]`** row, or **`none — no prior desktop tool in this thread`**.
The **lookup table** judges **that** action only (see **Ground rules** — do not invent prior actions).

**Before vs after (image anchor)** — **Only** **`Verify:`** line that **re-reads** screenshots.
UI delta uses **only** the full-screen pair:

- **When `[Screen before action]` exists:** compare **`[Screen before action]`** (pre-action layout) → **`[Screen after action]`** (post-action layout).
  **Before vs after** must **name both frames** — e.g. **`On [Screen before action]: … On [Screen after action]: …`**
  or **`same on [Screen before action] and [Screen after action]`**.
- **When `[Screen before action]` is absent** (first capture in thread): **`Before vs after: n/a — no [Screen before action]`**;
  do **not** invent a before frame.
- **Do not** use **`[Annotated after action]`** or zoom crops for this pair —
  **`Pointer:`** owns **`[Zoom pointer before action]`** geometry.

**Clear evidence** — **Do not** re-open frames or add new pixel facts.
Pick **`supporting_evidence`** · **`contradicting_evidence`** · **`no_clear_evidence`**
and **restate** the **Before vs after** conclusion in verdict terms
(match / mismatch / no visible change vs **Last automated action** intent).
**Forbidden:** a second **`On [Screen …]:`** inventory on **Clear evidence**.

**Analysis order (fixed)** — Fill inputs in order, then **look up** **Step result** + **`Cause`**.
**Do not** emit **`Outcome:`** in **`Verify:`** (**`Outcome:`** is **only** for **`Location:`** route lines).

1. **Before vs after** — frame-anchored delta (**only** image read in **Verify:**)
2. **Clear evidence** — label + **restate** **Before vs after** (**no** new image read)
3. **Action type** — **`deferred`** · **`non-deferred`** (short reason on the line)
4. **Mouse judgment** — **`non_mouse`** · **`mouse_miss`** · **`mouse_accurate`**
   (must match **`Pointer:`** **`3 Conclusion (Center-only rule)`**; see mapping below — **no** **`mouse_unknown`**)
5. **Lookup** — **one** row in the **decision table** → **`Step result`** + **`Cause`**

**First turn (outside table):** **`Last automated action:`** = **`none — no prior desktop tool in this thread`**
→ **`Step result: n/a`** · omit **`Cause:`** — do **not** invent a prior action or **`pass`**.

**1 — Clear evidence** (from **Before vs after** only)

| Value | When |
|-------|------|
| **`supporting_evidence`** | **Before vs after** shows the **Last automated action** intent **succeeded** on canvas (restate that delta — do not re-describe pixels). |
| **`contradicting_evidence`** | **Before vs after** **contradicts** intent (wrong panel/app, error blocks goal — restate the mismatch). |
| **`no_clear_evidence`** | **Before vs after** shows **no visible outcome** for what the action should have changed (restate “same” / unchanged). |

**2 — Action type**

- **`deferred`** — Pass/fail **not** settled on this screenshot alone
  (download/upload/export/queue/sync/save-to-disk/background).
  **`pending`** only with **`no_clear_evidence`** + valid mouse judgment (not **`mouse_miss`**).
- **`non-deferred`** — Expect an **immediate on-canvas** change (dialog, toggle, focus, validation, scroll, new row, submit feedback, etc.).

**Off-frame inspect** (per **§ Off-frame tools (rare)**) — only after **`Step result: pending`** on a **valid** deferred trigger —
**never** right after **`mouse_miss`** / **`precision_miss`** (re-aim first).

**3 — Mouse judgment** (from **`Pointer:`** + last tool)

| Value | When |
|-------|------|
| **`non_mouse`** | Last action has **no** precision click geometry (**`hotkey`**, **`wait`**, **`scroll`**, …) — **`Pointer:`** **`n/a`**. |
| **`mouse_miss`** | Precision click and **`Pointer:`** **`abnormal`** (hotspot vs center on **`[Zoom pointer before action]`** when present). |
| **`mouse_accurate`** | Precision click and **`Pointer:`** **`accurate`**.
  Post-action UI change does **not** yield a fourth mouse label — use **before** for geometry. |

**4 — Decision table (exactly one row)**

| **Clear evidence** | **Action type** | **Mouse judgment** | **Step result** | **Cause** |
|--------------------|---------------|--------------------|-----------------|----------|
| **`contradicting_evidence`** | either | **`mouse_miss`** | **`fail`** | **`precision_miss`** |
| **`contradicting_evidence`** | either | **`mouse_accurate`** | **`fail`** | **`wrong_operation`** |
| **`contradicting_evidence`** | either | **`non_mouse`** | **`fail`** | **`wrong_operation`** |
| **`supporting_evidence`** | either | **`mouse_miss`** | **`fail`** | **`precision_miss`** |
| **`supporting_evidence`** | either | **`mouse_accurate`** | **`pass`** | — |
| **`supporting_evidence`** | either | **`non_mouse`** | **`pass`** | — |
| **`no_clear_evidence`** | **`non-deferred`** | **`mouse_miss`** | **`fail`** | **`precision_miss`** |
| **`no_clear_evidence`** | **`non-deferred`** | **`mouse_accurate`** | **`fail`** | **`no_immediate_feedback`** |
| **`no_clear_evidence`** | **`non-deferred`** | **`non_mouse`** | **`fail`** | **`no_immediate_feedback`** |
| **`no_clear_evidence`** | **`deferred`** | **`mouse_miss`** | **`fail`** | **`precision_miss`** |
| **`no_clear_evidence`** | **`deferred`** | **`mouse_accurate`** | **`pending`** | **`off_frame_unverified`** |
| **`no_clear_evidence`** | **`deferred`** | **`non_mouse`** | **`pending`** | **`off_frame_unverified`** |

**Rules**

- **`wrong_operation`** only when the table row says so —
  requires **`mouse_accurate`** or **`non_mouse`** with **`contradicting_evidence`**, **never** **`mouse_miss`**.
- **`pass`** = **this** **Last automated action** succeeded on evidence (**`supporting_evidence`** + not **`mouse_miss`**).
  **Overall** user task may still be incomplete — see **Full completion** before **`response`**.
- **`pending`** only from the **three** **`deferred`** + **`no_clear_evidence`** rows —
  **never** when unsure; use **`no_immediate_feedback`** on **`non-deferred`**.

**After lookup:** **`Next:`** line 1 + **`Tool kind`** follow **§4 Verify → Next** for the same **`Step result`** / **`Cause`**.

**Required form**

```text
Verify:
Last automated action:
  <newest [Recent desktop tool calls] row — tool:method — summary
   | none — no prior desktop tool in this thread>.
Before vs after:
  <On [Screen before action]: … On [Screen after action]: …
   | same on both frames
   | n/a — no [Screen before action]>
  — only line that cites frames for UI delta.
Clear evidence:
  <supporting_evidence | contradicting_evidence | no_clear_evidence>
  — <restate Before vs after verdict; no new On [Screen …]: facts>.
Action type: <deferred | non-deferred> — <reason>.
Mouse judgment:
  <non_mouse | mouse_miss | mouse_accurate>
  — <must match Pointer 3 Conclusion;
     cite On [Zoom pointer before action]: when present for hotspot geometry>.
Prior tool text (if any): <role only; no secrets> | omit | none.
Step result: <pass | fail | pending | n/a>.
Cause:
  <wrong_operation | precision_miss | no_immediate_feedback | off_frame_unverified
   — omit when pass; n/a when Step result is n/a>.
```

If no textual payload exists for the last action, either omit **`Prior tool text (if any):`**
or write **`Prior tool text (if any): none.`**

In real **`Verify:`** replies, each labeled field above is **one physical line**
(join wrapped template sub-lines if needed).

**Mini examples — lookup rows (+ anti-pattern)**

Unless a block is explicitly marked **truncated anti-pattern**,
assume full **`Verify:`** form is required in real replies.

**Forbidden — vague Before vs after (no frame anchors):**

```text
Verify:
Before vs after: Text appears in search bar.
```

(truncated anti-pattern — shown only to illustrate the frame-anchor violation)

**Forbidden — Clear evidence re-reads frames (duplicate pixels):**

```text
Before vs after: On [Screen before action]: search bar empty. On [Screen after action]: text “老婆” in search bar.
Clear evidence: supporting_evidence — On [Screen after action]: typed text visible in search bar.
```

(truncated anti-pattern — shown only to illustrate Clear evidence re-reading pixels)

**Correct — Before vs after names frames; Clear evidence only restates that delta.**

```text
Verify:
Last automated action: 2. composite_action:type_text_at_index — typed “老婆” in search bar.
Before vs after: On [Screen before action]: search bar empty. On [Screen after action]: text “老婆” in search bar.
Clear evidence: supporting_evidence — restates Before vs after: typed text appeared as intended.
Action type: non-deferred.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on search field center.
Step result: pass.
```

**Minimal full-form click example (common path; includes `Prior tool text`)**

```text
Verify:
Last automated action: 1. mouse:click_index — Search button in toolbar.
Before vs after: On [Screen before action]: search panel closed. On [Screen after action]: search panel opened.
Clear evidence: supporting_evidence — restates Before vs after: search panel opened as intended.
Action type: non-deferred.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Search button center.
Prior tool text (if any): none.
Step result: pass.
```

**Correct — export click wrong panel (lookup row):**

```text
Verify:
Last automated action: 2. mouse:click_index — Export in toolbar.
Before vs after: On [Screen before action]: export toolbar idle. On [Screen after action]: History panel open instead of export flow.
Clear evidence: contradicting_evidence — restates Before vs after: wrong panel vs export intent.
Action type: non-deferred.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Export toolbar center.
Step result: fail.
Cause: wrong_operation.
```

```text
Verify:
Last automated action: 1. mouse:click_index — Export in toolbar.
Before vs after: On [Screen before action]: export toolbar idle. On [Screen after action]: History panel open.
Clear evidence: contradicting_evidence — restates Before vs after: wrong panel vs export intent.
Action type: non-deferred.
Mouse judgment: mouse_miss — On [Zoom pointer before action]: hotspot on History chip, not Export center.
Step result: fail.
Cause: precision_miss (forbidden: wrong_operation).
```

```text
Verify:
Last automated action: 1. mouse:click_index — trash icon on list row.
Before vs after: On [Screen before action]: row unchanged. On [Screen after action]: same row; trash row unchanged.
Clear evidence: no_clear_evidence — restates Before vs after: no visible delete effect.
Action type: non-deferred.
Mouse judgment: mouse_miss — On [Zoom pointer before action]: hotspot on row text, not trash icon center.
Step result: fail.
Cause: precision_miss.
```

**Mini example — pending after hotkey save (deferred, full form)**

```text
Verify:
Last automated action: 4. hotkey — Save document (Ctrl+S).
Before vs after: On [Screen before action]: same document canvas. On [Screen after action]: same canvas; no “Saved” toast on frame.
Clear evidence: no_clear_evidence — restates Before vs after: no save confirmation on canvas.
Action type: deferred — proof off-frame or later frame.
Mouse judgment: non_mouse — hotkey; Pointer n/a.
Step result: pending. Cause: off_frame_unverified.
```

**Anti-pattern — deferred + `mouse_miss` → must not be `pending`**

```text
Verify:
Last automated action: 3. mouse:click_index — download icon on attachment row.
Before vs after: On [Screen before action]: attachment list unchanged. On [Screen after action]: same list; no progress on canvas.
Clear evidence: no_clear_evidence — restates Before vs after: no download progress on canvas.
Action type: deferred.
Mouse judgment: mouse_miss — On [Zoom pointer before action]: hotspot on filename text, not download icon center.
Step result: fail.
Cause: precision_miss (forbidden: pending).
```

```text
Verify:
Before vs after: On [Screen before action]: Submit enabled; no new message. On [Screen after action]: same; Submit still enabled; no new message.
Clear evidence: no_clear_evidence — restates Before vs after: submit not confirmed.
Action type: non-deferred.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Submit pill center.
Step result: fail.
Cause: no_immediate_feedback.
```

```text
Verify:
Last automated action: 1. mouse:click_index — Export in export list.
Before vs after: On [Screen before action]: export list at “2 of 10”. On [Screen after action]: progress “3 of 10 complete”.
Clear evidence: supporting_evidence — restates Before vs after: export batch advanced.
Action type: non-deferred.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Export control center.
Step result: pass.
```

```text
Verify:
Last automated action: 2. mouse:click_index — Export in toolbar.
Before vs after: On [Screen before action]: idle export control. On [Screen after action]: spinner started; main canvas unchanged.
Clear evidence: no_clear_evidence — restates Before vs after: spinner only; queue proof not on canvas.
Action type: deferred — queue proof off-frame.
Mouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Export center.
Step result: pending.
Cause: off_frame_unverified.
```

```text
Verify:
Last automated action: 1. mouse:click_index — sidebar toggle.
Before vs after: On [Screen before action]: same idle page; sidebar closed. On [Screen after action]: same idle page; no new sidebar.
Clear evidence: no_clear_evidence — restates Before vs after: sidebar still closed.
Action type: non-deferred.
Mouse judgment: mouse_miss — On [Zoom pointer before action]: hotspot on History chip, not sidebar toggle center.
Step result: fail.
Cause: precision_miss.
```

**Mini example — save dialog after Ctrl+S (`supporting_evidence` + `pass`)**

```text
Verify:
Last automated action: 3. hotkey — Save document (Ctrl+S).
Before vs after: On [Screen before action]: document canvas only. On [Screen after action]: save modal appeared on canvas.
Clear evidence: supporting_evidence — restates Before vs after: save modal appeared on canvas.
Action type: non-deferred — dialog on canvas.
Mouse judgment: non_mouse — hotkey turn.
Step result: pass.
```

---

### 3) Repetition

**`[Recent desktop tool calls]`** (oldest → newest; last = latest): same **goal+action** with no UI gain? Count **consecutive** rows. **>3** → **`STUCK`**. Sameness = **text**, not index.

**Required form**

```text
Repetition:
Rows: <same goal|action × N | differ>.
Screen: <flat | advanced — unindexed>.
Verdict: <OK | STUCK> — <if STUCK: one tactic hint, no digits>.
```

**Mini examples — verdict branch**

```text
Repetition:
Rows differ; after advanced. OK
```

```text
Repetition:
Rows: same goal|action × 4 consecutive; after flat. Verdict: STUCK — use coordinates or different control, not same click_index pattern.
```

**Mini example — rows differ but screen flat (still OK)**

```text
Repetition:
Rows: goal text differs last row vs prior. Screen: flat. OK — not same semantic repeat.
```

---

### 4) Next

**`Next:` chain (two numbered lines + `Tool kind`)** — Use the **same numbered-line discipline** as **§5 Target Locating**. **Do not** put overlay **`index`**, zoom digits, or “bbox N” here (**only** in **`Location:`**). Complete JSON examples are under **Full chain**.

1. **Prior stages & sub-goal** — **One** line that **restates** **`Verify:`** (verdict + **short** reason) · **`Repetition:`** (verdict) · **what this single turn advances** toward the user task. **No** overlay digits; **no** new evidence not already in **`Pointer:`** / **`Verify:`** / **`Repetition:`**.

2. **Target on `[Screen after action]`** — **One** **operationally clear** aim **on the current post-action full-screen inject only**:
   name the **control or row** you will use, with **visible** **label** (exact or partial text, or “unlabeled icon”),
   **shape** (pill, chip, row, tab, field, glyph), **color / emphasis** if it disambiguates, **band / region**, and **neighbors**.
   You may **prefix** sub-clauses with **`On [Screen after action]:`** for each facet group.
   Traits must be **already visible and uniquely describable** on **`[Screen after action]`**; **no** **`[Annotated after action]`** here.
   **Do not** use **relationship** nicknames as the **label** **unless** that **exact** string appears on the frame.

**No speculative or procedural text in line 2** —
**Do not** use **modal** qualifiers (**“might”**, **“probably”**, **“could be labeled”**)
or **multi-phase** hunt language (**“locate … then identify”**, **“find the right row first”**, **“need to pick among …”**) in **line 2**.
If nothing is **yet** uniquely nameable on the frame, **line 2** must describe a **preparatory** visible target for **this** turn’s **`Tool kind`**
(**scroll** surface, **expand** chevron, **wait** region, etc.),
or **`Tool kind`** must be **inspect-only** (**`wait`**, **`scroll`**, **`hotkey`**, per **§ Off-frame tools (rare)**)
until a later turn can name a unique click target on **line 2**.
**`Tool kind`** is the **tool class only** — **no** search narrative there either.

**Verify → Next (line 1 + Tool kind)** — After the **lookup table**, read **`Step result`** + **`Cause`**; **line 1** must restate them and follow the matching row below (no overlay digits).

| **Step result** | **Cause** | **Next line 1 must…** | **Tool kind** (typical) |
|-----------------|-----------|------------------------|-------------------------|
| **`pass`** | — | Advance the **next** sub-goal toward the user task; may use **`response`** only if **Full completion** is met — **`pass`** on one action does not suffice alone. | **mouse** / **hotkey** / **composite_action** on visible target |
| **`fail`** | **`wrong_operation`** | **Pivot** — different surface, panel, or tactic (**only** after geometry gate passed); **do not** off-frame inspect to excuse a visible miss. | Different visible control — **not** repeat same wrong path |
| **`fail`** | **`precision_miss`** | **Re-aim** the **same** intent (**`Location:`** / coordinates); cite **`fail — precision_miss`**. | **mouse click** / **coordinates** — **not** off-frame inspect first |
| **`fail`** | **`no_immediate_feedback`** | **Retry or unblock** — **`wait`**, **scroll**, alternate control, or second attempt; re-check **Action type** if canvas truly cannot show proof yet. | **`wait`** · **scroll** · **mouse click** |
| **`pending`** | **`off_frame_unverified`** | **Inspect only** — **`wait`**, status/history, queue/folder, **scroll**; **no** repeating the **same** trigger until **`pass`** or **`fail`**. | **`wait`** · **scroll** · off-frame per **§ Off-frame tools (rare)** when earned |
| **`n/a`** | — | First turn or no prior action — open task from user goal; **no** invented **`pass`**. | Per user task on visible UI |

**Required form**

```text
Next:
1 Prior stages & sub-goal: Verify: <Step result> — <Cause or short reason>; Repetition: <OK | …>; this turn: <one concrete advance — no overlay digits>.
2 Target on [Screen after action]: <label or unlabeled icon; shape; color if needed; band/region; neighbors — visible on this frame only>.
Tool kind: <e.g. mouse click | scroll | wait | hotkey | response — tool class only; no digits; no search narrative>.
```

**Mini example — anti-pattern (line 2 must not read like a hunt)**

```text
Next:
1 Prior stages & sub-goal: Verify: pass; Repetition: OK; this turn: open the intended personal chat from the sidebar.
2 Target on [Screen after action]: locate a specific person’s thread — might be a personal chat; look for familiar avatar or name.
Tool kind: mouse click — need to identify the right row first.
```

**Mini example — tight line 2 when the row title is literally on-frame**

```text
Next:
1 Prior stages & sub-goal: Verify: pass; Repetition: OK; this turn: open one chat row by its on-screen title.
2 Target on [Screen after action]: sidebar chat row whose **visible title text** matches the on-frame spelling (example: “Alice”); shape list row with avatar + title; band left chat list; neighbors: under the search field if visible.
Tool kind: mouse click — no digits here.
```

**Mini examples — branch**

**Mini example — anti-pattern (fabricated prior action + premature `response`)**

```text
Pointer:
1 Intended aim on [Screen after action]: WeChat dock icon; aim = icon center.
2 Evidence (hotspot vs aim): On [Zoom bottom after action]: WeChat tile visible — (forbidden: no prior click to judge hotspot vs center).
3 Conclusion (Center-only rule): n/a — invented “first action” rule (forbidden).

Verify:
Tool reply present. Step result: pass (forbidden format + no Last automated action line).

Next:
1 Prior stages & sub-goal: Verify: pass — user task complete (forbidden: WeChat not open; no [Recent desktop tool calls] row).
Tool kind: response (forbidden on first turn while app still closed).
```

**Correct — first turn, no `[Recent desktop tool calls]`**

```text
Pointer:
1 Intended aim on [Screen after action]: n/a — no prior automated action to judge (no [Recent desktop tool calls] row).
2 Evidence (hotspot vs aim): On [Screen after action]: n/a — no prior click hotspot to compare.
3 Conclusion (Center-only rule): n/a — no last automated action in thread.

Verify:
Last automated action: none — no prior desktop tool in this thread.
Before vs after: n/a — no [Screen before action] (first capture).
Clear evidence: n/a — no prior action.
Action type: n/a — no prior action.
Mouse judgment: n/a — agrees with Pointer 3 Conclusion.
Step result: n/a — no prior action to judge (do not invent prior actions or pass).

Next:
1 Prior stages & sub-goal: Verify: n/a — no prior action; Repetition: OK; this turn: open WeChat from dock per user task.
2 Target on [Screen after action]: WeChat app icon in dock; shape square app tile; band bottom dock; neighbors: adjacent dock icons; red badge on tile if visible.
Tool kind: mouse click — no digits here.
```

```text
Next:
1 Prior stages & sub-goal: Verify: fail — no_immediate_feedback; Repetition: OK; this turn: focus email field to correct address.
2 Target on [Screen after action]: email text field with red outline; shape single-line input; band signup form stack; neighbors: under “Email” label, above password field.
Tool kind: mouse click — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: fail — precision_miss; Repetition: OK; this turn: re-click download icon center on same attachment row.
2 Target on [Screen after action]: unlabeled download glyph on attachment row; shape small square icon; band list row right; neighbors: filename text cell to the left of icon.
Tool kind: mouse click — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: pending — off_frame_unverified; Repetition: OK; this turn: pause for OS save indicator.
2 Target on [Screen after action]: n/a — no click target; allow repaint after Ctrl+S.
Tool kind: wait — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: pass — export batch progressing; Repetition: OK; this turn: scroll export list to expose remaining rows.
2 Target on [Screen after action]: vertical scroll track on file list panel; shape narrow scrollbar; band center-right of export dialog; neighbors: bottom rows clip at panel edge.
Tool kind: scroll — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: pass; Repetition: OK; this turn: dismiss success snackbar.
2 Target on [Screen after action]: label “×” or short “Done” if visible; shape slim horizontal banner; color green emphasis; region top of page canvas; neighbors: below title/tabs strip, above main content.
Tool kind: mouse click — no digits.
```

**Mini example — full trait checklist on line 2 (before Target Locating)**

```text
Next:
1 Prior stages & sub-goal: Verify: fail — no_immediate_feedback; Repetition: OK; this turn: retry submit from modal.
2 Target on [Screen after action]: label “Submit” or unlabeled gray pill; shape pill; color gray; region modal dialog center stack; neighbors: under password fields, full column width — not Cancel text link.
Tool kind: mouse click — no digits.
```

---

### 5) Target Locating

#### Purpose

**`Location:`** turns **`Next:`** **line 2** (the **`[Screen after action]`** target traits) into a **bounded, auditable** choice of
**`index`** / **coordinates** / **hover** on the **annotated / zoom** overlays.
It is an **observation + routing** layer: it **does not** replace **`Next:`** line 2 wording;
it **paraphrases** on-screen overlay evidence and records **why** one candidate wins.

- Ground **`Next:`** **line 2** only — **do not** paste **line 2** verbatim into **`Location:`**; paraphrase overlay-visible facts.
  **`Next:`** **line 1** is **context only** — **do not** paste **line 1** into **`Location:`**.
- Reference frames: **`[Annotated after action]`** | **`[Zoom top after action]`** | **`[Zoom bottom after action]`** | **`[Zoom pointer after action]`** —
  pick **after** **placement / bearing** on **`[Screen after action]`** (see **Derivation chain** step 1), **not** by habit.
- **Do not** jump straight to “use **`index`** N” — follow the chain below end-to-end.
- **Evidence before conclusion** — same discipline as **Stepwise derivation**:
  each **`Location:`** line builds **observations first**, then **one** closing label
  (**therefore selected overlay index N**, **`single`**, **`index` N** on **Outcome**, etc.).
  **Line 1** must **not** name any overlay **`index`** — neighbor **“(index 34)”** is a **conclusion**, not observation.
  **Never** open a line with the final **`index`** / route / tool choice and backfill reasons afterward.

**Derivation chain (mandatory)**

1. **Placement (bearing) → reference frame.** From **`Next:`** **line 2** **band / region / neighbors**
   (grounded on **`[Screen after action]`** layout), state **where** the target sits on the full screen —
   e.g. **top** menu/title/tab strip, **bottom** dock/taskbar, **beside synthetic pointer** in a list/dialog row, **central** canvas/dialog body.
   **Then** **`therefore analyze on […]`** — pick **one** overlay frame **before** **`bbox`** / **`index`** work:

   | Bearing on **`[Screen after action]`** | Prefer frame |
   |----------------------------------------|----------------|
   | **Top** band — menu bar, window title, tabs under chrome | **`[Zoom top after action]`** when overlay digits are small/crowded; else **`[Annotated after action]`** |
   | **Bottom** band — OS dock, taskbar, launcher strip | **`[Zoom bottom after action]`** |
   | **Near synthetic pointer** — row chip, inline control under cursor | **`[Zoom pointer after action]`** when clearest |
   | **Wide / central** — dialog stack, full-width toolbar, multi-control panel | **`[Annotated after action]`** |

   **Line 1** must open with this **placement → therefore frame** clause (still **no** overlay **`index`** on that clause).

2. **Target features → `bbox` on that frame (evidence, then conclude `index`).** On the **chosen** frame only: target traits + **wrapping `bbox`** —
   **no** overlay **`index`** (**forbidden** in **`neighbors:`**).
   **Line 2:** **(a)(b)(c)** on the **same** frame — **only then** **`therefore selected overlay index N`**.
   Do **not** write **`index` N** at the **start** of **line 2**.
   If the frame was wrong (target not visible / digits unreadable), **discard** and restart **step 1** with a different bearing→frame choice.

3. **Wrap count → exclusivity → route.** Count **distinct actionable elements** in **line 1** **`inventory:`**: **total = 1** ⇒ **`single`** ⇒ **index path**; **total > 1** ⇒ **`multiple`** ⇒ **coordinate path**.

4. **`single` (exclusive)** ⇒ **Outcome: `index` N`** (same **`N`** as **line 2**).

5. **`multiple` (non-exclusive)** ⇒ **Outcome: `coordinates`** —
   anchor on **`[Zoom pointer after action]`** when the sub-target appears in the pointer zoom;
   else **`[Screen after action]`** or the **line 1** frame.

#### Template (multi-line template + analysis flow)

Follow **Derivation chain** above for bearing→frame choice and overall order.
**Evidence before conclusion** on **every** numbered line (same as **Stepwise derivation** in **Ground rules**).

**Required form — `Placement→frame` through `Outcome`**

Each label below is **one physical line** in output (join sub-clauses onto that line if needed).
Internalize **`Next:`** line 2 as the search spec — **do not** paste **`Next:`** line 1 or verbatim line 2 into **`Location:`**.

**`1 Placement→frame:`** (observations only — **no** overlay **`index`** anywhere, including **`neighbors:`**)

- **`On [Screen after action]:`** bearing — **top | bottom | near pointer | central** + band/neighbors from **`Next:`** line 2.
- **`→ therefore analyze on`** one overlay frame (see bearing table in **Derivation chain**).
- On **that frame** — target on overlay (paraphrase **`Next:`** line 2): text, shape, color, band, neighbors — **zero overlay digits**.
- **`wrapping bbox:`** anchor + border stroke color.
- **`traits inside that bbox:`** …
- **`inventory:`** distinct actionable elements inside the paired **`bbox`** — names only.

**`2 … target→bbox→index:`** (same bracketed frame as line 1 — **`N` only as the last token group**)

- **(a)** line 1 wrapping **`bbox`** border color + anchor.
- **(b)** candidate digit **background** matches; **only** flush-adjacent to **that** **`bbox`** (not between two **`bbox`** regions).
- **(c)** that **`bbox`** wraps the line 1 target.
- **`therefore selected overlay index N`** — only if **(a)–(c)** hold.
- If **(b)** pairs a **different** **`bbox`**: **`discard trial — no index selected`** — **no** lines **3–4**; retry lines **1–2** (another digit, or restart bearing→frame).

**`3 Exclusivity:`** (from line 1 **`inventory:`** wrap count — evidence before route name)

- **`inventory`** → wrap count → **`single`** (count = 1) | **`multiple`** (count > 1) → **`route: index path`** | **`route: coordinate path`**.

**`4 Outcome:`** (open with **route from line 3** — **forbidden** to lead with **`index` N** or bare **`(x, y)`**)

- **`single` / index path:** **`selected overlay index N`** — same **`N`** as line 2.
- **`multiple` / coordinate path:** **(a)** anchor frame — **`[Zoom pointer after action]`** when sub-target visible there,
  else **`[Screen after action]`** or line 1 frame; **(b)** sub-target placement on that frame vs synthetic pointer / layout;
  **(c)** **`therefore`** **(x, y)** in session scale per **Pointer position** — not overlay **`index` N** for click.
- **No valid digit after all trials:** one exhausted **line 4** with tactic / coordinates / hover and why — **not** one line 4 per failed digit.

```text
Location:
1 Placement→frame:
   On [Screen after action]: <bearing + band/neighbors>
   → therefore analyze on <overlay frame>.
   <Same frame> — target on overlay: <traits; no digits>.
   wrapping bbox: <color + anchor>.
   traits inside bbox: <…>.
   inventory: <names only>.
2 On <same frame as line 1> — target→bbox→index:
   (a) … (b) … (c) … therefore selected overlay index <N>.
3 Exclusivity:
   inventory … — wrap count …; <single|multiple>; route: <index|coordinate> path.
4 Outcome:
   route … — <index N | coordinates (a)(b)(c) therefore (x,y) | exhausted tactic>.
```

**Overlay trials — discard, retry, and when `index` may appear**

- Work **one overlay trial** at a time.
- After the **first** valid lines **1–2** pair, append lines **3–4** **once** only.
- Overlay digit **`N`** may appear **only** at the end of a successful line **2** and on line **4** when the **index** route wins — nowhere else in **`Location:`**.

**Forbidden patterns**

- Paste **`Next:`** line 1 or verbatim line 2 into **`Location:`**.
- Open line 1 on a zoom/annotated frame **without** bearing on **`[Screen after action]`** first.
- Cite overlay **`index`** on line 1 (**including** **`neighbors:`** like “**(index 34)**”) — that pre-decides **`N`** before **(a)–(c)**.
- Open line 2 with **`index` N`**, or line 4 with **`index` N** / bare **`(x, y)`** before **route** and coordinate anchor evidence.
- Add **`match` / `mismatch` vs Next line 2**, or emit lines **3–4** after a **discarded** line 2.

#### Cases (template + examples)

**Line 1 only — target + wrapping bbox + traits (no `index` yet)**

(one on-wire **line 1**; sub-clauses shown on separate lines for readability)

```text
Location:
1 Placement→frame:
   On [Screen after action]: app window **footer** band above OS dock (central-bottom of dialog, not taskbar)
   → therefore analyze on **[Annotated after action]**.
   [Annotated after action] — target on overlay (paraphrase **Next** line 2):
   Settings gear icon; shape gear glyph; color gray; band footer;
   neighbors: red-border label strip **immediate left** of icon (layout names only, no overlay numerals);
   wrapping bbox: **green**-stroke region around footer gear icon;
   traits inside bbox: visible gray gear glyph;
   inventory: gear icon only.
```

**Anti-pattern — line 1 cites neighbor `index` (forbidden; equals early conclusion)**

```text
Location:
1 Placement→frame: On [Screen after action]: **bottom** OS dock band (forbidden: skipped bearing, jumped to frame). [Zoom bottom after action] — target on overlay: … neighbors: **left of Messages (index 34)** … (forbidden — overlay digits on line 1).
```

**Correct line 1 — dock target: bearing → bottom zoom, then overlay detail**

```text
Location:
1 Placement→frame: On [Screen after action]: target in **bottom** launcher strip (dock/taskbar), not main canvas → therefore analyze on **[Zoom bottom after action]**. [Zoom bottom after action] — target on overlay (paraphrase **Next** line 2): WeChat app icon in dock; shape square app tile; color green tile with chat bubble glyph; band bottom dock; neighbors: **Messages** square tile **immediate left**, **App Store** blue tile **immediate right** (labels/shapes only — no overlay digits); wrapping bbox: **blue**-stroke region hugging WeChat icon only; traits inside bbox: green WeChat tile with red notification badge; inventory: WeChat icon only.
```

**Lines 1–2 — add derived `index` + target→bbox→index proof**

```text
Location:
1 Placement→frame: On [Screen after action]: app **footer** band (dialog chrome, not OS dock) → therefore analyze on **[Annotated after action]**. [Annotated after action] — target on overlay (paraphrase **Next** line 2): Settings gear icon; shape gear glyph; color gray; band footer; neighbors: red-border label strip **immediate left** of icon (layout names only, no overlay numerals); wrapping bbox: **green**-stroke region around footer gear icon; traits inside bbox: visible gray gear glyph; inventory: gear icon only.
2 On [Annotated after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **green** around footer gear icon; (b) on frame, overlay digit **background** **green**, **only** flush-adjacent to that **green**-stroke **bbox** (red-border neighbor **left** excluded — stroke mismatch); (c) **bbox** wraps line 1 gray gear target; **therefore** selected overlay index **11**.
```

**Discarded trial — index pairs wrong `bbox` (lines 1–2 only; retry)**

```text
Location:
1 Placement→frame: On [Screen after action]: app **footer** band → therefore analyze on **[Annotated after action]**. [Annotated after action] — target on overlay (paraphrase **Next** line 2): Settings gear in footer; shape gear; band footer; wrapping bbox: **green**-stroke region around footer gear icon; traits inside bbox: gray gear glyph; inventory: gear icon only.
2 On [Annotated after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **green** around footer gear icon; (b) on frame, **green**-background digit flush on tall **green**-stroke rail/card stack — **not** line 1 wrapping **bbox**; discard trial — no index selected.
```

**Lines 1–3 — `single` (full through Exclusivity)**

```text
Location:
1 Placement→frame: On [Screen after action]: ⋯ chip **beside synthetic pointer** in a list row (mid canvas) → therefore analyze on **[Zoom pointer after action]**. [Zoom pointer after action] — target on overlay (paraphrase **Next** line 2): ⋯ chip target; shape pill; band row title area; neighbors: **right** of row title text; wrapping bbox: **magenta**-stroke region hugging ⋯ chip **right** of title; traits inside bbox: text ⋯, pill, row title band, **right** of title; inventory: ⋯ chip only.
2 On [Zoom pointer after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **magenta** hugging ⋯ chip **right** of row title; (b) on frame, overlay digit **background** **magenta**, **only** flush-adjacent to that **magenta**-stroke **bbox**; (c) **bbox** wraps line 1 ⋯ chip target; **therefore** selected overlay index **4**.
3 Exclusivity: inventory ⋯ chip only — wrap count **1**; **single**; route: **index** path.
```

**Lines 1–3 — `multiple`**

```text
Location:
1 Placement→frame: On [Screen after action]: URL field in **top** browser chrome under tab row → therefore analyze on **[Zoom top after action]**. [Zoom top after action] — target on overlay (paraphrase **Next** line 2): URL field in toolbar; shape text input; band toolbar under tabs; neighbors: **left** of star bookmark; wrapping bbox: wide **orange**-stroke toolbar strip under tab row; traits inside bbox: URL string visible; text-input chrome; toolbar under tabs; **left** of star; inventory: URL field + star + extensions.
2 On [Zoom top after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **orange** (wide toolbar under tabs); (b) on frame, overlay digit **background** **orange**, **only** flush-adjacent to that **orange**-stroke toolbar **bbox**; (c) **bbox** wraps line 1 URL-field target among toolbar controls; **therefore** selected overlay index **12**.
3 Exclusivity: inventory URL field + star + extension icons — wrap count **3**; **multiple**; route: **coordinate** path.
```

**Line 4 — `index` after `single`**

```text
Location:
1 Placement→frame: On [Screen after action]: ⋯ chip **near synthetic pointer** in list row → therefore analyze on **[Zoom pointer after action]**. [Zoom pointer after action] — target on overlay (paraphrase **Next** line 2): ⋯ chip target; shape pill; band row title area; neighbors: **right** of row title text; wrapping bbox: **magenta**-stroke region hugging ⋯ chip **right** of title; traits inside bbox: text ⋯, pill, row title band, **right** of title; inventory: ⋯ chip only.
2 On [Zoom pointer after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **magenta** hugging ⋯ chip; (b) on frame, overlay digit **background** **magenta**, **only** flush-adjacent to that **magenta**-stroke **bbox**; (c) **bbox** wraps line 1 ⋯ chip target; **therefore** selected overlay index **4**.
3 Exclusivity: inventory ⋯ chip only — wrap count **1**; **single**; route: **index** path.
4 Outcome: route **index** path (line 3 **single**) — selected overlay index **4**
```

**Line 4 — `coordinates` after `multiple` (dialog footer)**

```text
Location:
1 Placement→frame: On [Screen after action]: OK/Cancel in **central** modal footer (wide footer bar, not OS chrome) → therefore analyze on **[Annotated after action]**. [Annotated after action] — target on overlay (paraphrase **Next** line 2): OK primary button; shape blue pill; band dialog footer; neighbors: **left** of Cancel pill; wrapping bbox: **cyan**-stroke footer bar spanning **OK** + **Cancel**; traits inside bbox: “OK” label on blue pill; dialog footer band; **left** of Cancel pill; inventory: OK pill + Cancel pill.
2 On [Annotated after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **cyan** (footer **OK**+**Cancel**); (b) on frame, overlay digit **background** **cyan**, **only** flush-adjacent to that **cyan**-stroke footer **bbox**; (c) **bbox** wraps line 1 OK pill among footer controls; **therefore** selected overlay index **4**.
3 Exclusivity: inventory OK pill + Cancel pill — wrap count **2**; **multiple**; route: **coordinate** path.
4 Outcome: route **coordinate** path (line 3 **multiple**) — **coordinates** — (a) anchor: **[Zoom pointer after action]** — OK pill and pointer hotspot both visible on pointer zoom crop; (b) OK pill center **below-left** of synthetic pointer on that crop; (c) **therefore** aim (x, y) ≈ (…, …) in session scale per **Pointer position** — not overlay index **4** for click
```

**Line 4 — `coordinates` after `multiple` (wide toolbar — top zoom)**

```text
Location:
1 Placement→frame: On [Screen after action]: URL field in **top** toolbar under tabs → therefore analyze on **[Zoom top after action]**. [Zoom top after action] — target on overlay (paraphrase **Next** line 2): URL field in toolbar; shape text input; band toolbar under tabs; neighbors: **left** of star bookmark; wrapping bbox: wide **orange**-stroke toolbar strip under tab row; traits inside bbox: URL string visible; text-input chrome; toolbar under tabs; **left** of star; inventory: URL field + star + extensions.
2 On [Zoom top after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **orange** (wide toolbar under tabs); (b) on frame, overlay digit **background** **orange**, **only** flush-adjacent to that **orange**-stroke toolbar **bbox**; (c) **bbox** wraps line 1 URL-field target among toolbar controls; **therefore** selected overlay index **12**.
3 Exclusivity: inventory URL field + star + extension icons — wrap count **3**; **multiple**; route: **coordinate** path.
4 Outcome: route **coordinate** path (line 3 **multiple**) — **coordinates** — (a) anchor: **[Zoom top after action]** — URL field not in pointer zoom; sub-target on top-toolbar crop from line 1; (b) URL field center **right** of star bookmark on that frame; (c) **therefore** aim (x, y) ≈ (…, …) in session scale — not overlay index **12** for click
```

---

### Full chain (complete JSON)

**Screen capture** is injected by the runtime, not a **`tool_name`** in your reply; the five-stage reasoning still lives in the **`thoughts`** string field.

**Screen-targeted tool** (includes **`Location:`**):

```json
{
  "thoughts": "Pointer:\n1 Intended aim on [Screen before action]: gray Submit pill in modal dialog center stack; aim = pill center.\n2 Evidence (hotspot vs aim): On [Zoom pointer before action]: synthetic pointer tip over **Submit pill interior**, hotspot overlaps pill **geometric center** vs **intended Submit pill center** — not on rim.\n3 Conclusion (Center-only rule): accurate — hotspot overlaps **Submit pill geometric center** on **[Zoom pointer before action]**.\n\nVerify:\nLast automated action: 2. mouse:click_index — Submit in modal.\nBefore vs after: On [Screen before action]: banner + Submit unchanged. On [Screen after action]: same; Submit still enabled; no new message.\nClear evidence: no_clear_evidence — restates Before vs after: submit not confirmed.\nAction type: non-deferred.\nMouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on Submit pill center.\nStep result: fail. Cause: no_immediate_feedback.\n\nRepetition:\nLast rows differ; not flat 4×. OK\n\nNext:\n1 Prior stages & sub-goal: Verify: fail — no_immediate_feedback; Repetition: OK; this turn: retry primary Submit from modal.\n2 Target on [Screen after action]: label \"Submit\" or unlabeled gray pill; shape pill; color gray; region modal dialog center stack; neighbors: under password fields — not Cancel text link.\nTool kind: mouse click — no digits here.\n\nLocation:\n1 Placement→frame: On [Screen after action]: Submit pill in **central** modal form stack (not top/bottom OS chrome) → therefore analyze on **[Annotated after action]**. [Annotated after action] — target on overlay (paraphrase **Next** line 2): gray pill labeled Submit or unlabeled gray pill; shape pill; color gray; band modal dialog center stack; neighbors: under password fields, Cancel text link present in strip adjacent to footer actions; wrapping bbox: tall **green**-stroke card bbox covering full form stack under modal title; traits inside bbox: visible gray pill labeled Submit under password fields; neighbor strip includes Cancel text link; inventory: email field, password fields, Submit pill.\n2 On [Annotated after action] — target→bbox→index: (a) line 1 wrapping **bbox** border **green** (tall card / full form stack); (b) on frame, overlay digit **background** **green**, **only** flush-adjacent to that **green**-stroke card **bbox**; (c) **bbox** wraps line 1 Submit pill among form fields; **therefore** selected overlay index **6**.\n3 Exclusivity: inventory email field + password fields + Submit pill + Cancel link strip — wrap count **4**; **multiple**; route: **coordinate** path.\n4 Outcome: route **coordinate** path (line 3 **multiple**) — **coordinates** — (a) anchor: **[Zoom pointer after action]** — Submit pill under pointer on pointer zoom crop; (b) pill center vs synthetic pointer on that crop; (c) **therefore** aim (x, y) ≈ (…, …) in session scale per **Pointer position** — not overlay index **6** for click",
  "headline": "Retry submit via coordinates",
  "tool_name": "mouse:click_at",
  "tool_args": {}
}
```

**Omitting `Location:`** — whenever the method does **not** choose a new **`index`** or **`x`/`y`** on the capture: e.g. **`wait`**, **`response`**, **`hotkey`**, **`clipboard:read`** / **`clipboard:write`**, **`mouse:…_current`**, **`move_offset`**, **`composite_action:type_text_at_focused`** — omit the whole **`Location:`** block.

Example (**`hotkey`** — confirm save dialog, no screen point):

```json
{
  "thoughts": "Pointer:\n1 Intended aim on [Screen after action]: n/a — prior hotkey Save aimed at document, not this dialog button.\n2 Evidence (hotspot vs aim): On [Screen after action]: n/a — judging keyboard confirm, not pointer vs dialog button center.\n3 Conclusion (Center-only rule): n/a — hotkey turn.\n\nVerify:\nLast automated action: 3. hotkey — Save document (Ctrl+S).\nBefore vs after: On [Screen before action]: document canvas only. On [Screen after action]: save modal appeared.\nClear evidence: supporting_evidence — restates Before vs after: save modal appeared.\nAction type: non-deferred — dialog on canvas.\nMouse judgment: non_mouse — hotkey; Pointer n/a.\nStep result: pass.\n\nRepetition:\nRows: differ. Screen: advanced. Verdict: OK\n\nNext:\n1 Prior stages & sub-goal: Verify: pass — save dialog open; Repetition: OK; this turn: confirm Save in dialog via keyboard.\n2 Target on [Screen after action]: n/a — default button focus; use keyboard confirm, not a new overlay pick.\nTool kind: hotkey — no digits here.",
  "headline": "Confirm save in dialog",
  "tool_name": "hotkey",
  "tool_args": {}
}
```