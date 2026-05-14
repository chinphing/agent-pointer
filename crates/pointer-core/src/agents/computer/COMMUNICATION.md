## On-wire shape: JSON object

Each desktop reply is **one** JSON object: string fields **`thoughts`**, **`headline`**, optional **`sidecar_tools`** array, then root **`tool_name`** and object **`tool_args`** (schema per tool prompt).

**`thoughts`** — Holds the **five-stage block** below (**`Pointer:`** through optional **`Location:`**), in order, using the **English prefix lines** only for that reasoning. Stages **1–4**: **no** overlay **`index`** / “bbox N”; **`index`** appears **only** inside **`Location:`** lines.

Complete examples at the end of this document use **full JSON**. Less important fields use **`...`**.

## Reasoning framework (every tool or final turn)

Run **five** stages **in order**. Use **exactly** these **English prefix lines**:

- **`Pointer:`** — stage 1 (**numbered lines `1`–`3` + `4 Conclusion (Center-only rule)`**)
- **`Verify:`** — stage 2  
- **`Repetition:`** — stage 3  
- **`Next:`** — stage 4 (**numbered lines `1`–`2`** + **`Tool kind`**)  
- **`Location:`** — stage 5 **only** when this turn’s method picks a **new** overlay **`index`** or screenshot **`x`/`y`** (or drag endpoints) from the current injects. **Omit** the whole **`Location:`** block for **`wait`**, **`clipboard`**, **`response`**, **`hotkey`**, **`mouse:…_current`**, **`move_offset`**, **`composite_action:type_text_at_focused`**, and any method that does **not** require those targets on the frame.

Within **each** stage, follow that stage’s **Required form** **top to bottom**; **do not** print **`Pointer:`** **`4 Conclusion (Center-only rule)`** / **`Location:`** **line 5** / other **Conclusion** / **Outcome** lines **before** the numbered lines that **earn** them (**Stepwise derivation** in **Ground rules**).

### Ground rules

**No speculation** — Evidence only from current **`[CUR_SCREEN]`** injects, **`[Recent desktop tool calls]`**, and **tool results already in this thread**. No success from memory or “usually…”. No clipboard claims without **`clipboard:read`** (or on-screen text). **Pointer hotspot** (synthetic mouse cursor) only from **`[Screen after action]`** (and zooms), not from intent; **`Pointer:`** does **not** judge **caret** / insertion bar.

**Image-grounded clauses** — Every **observation** in **`Pointer:`** **line 1** (when **both** before and after full-screen captures exist — **prefix** **`On [Screen before action]`** / **`On [Screen after action]`**), **`Pointer:`** **line 3**, **`Next:`** **line 2**, and **`Location:`** **lines 1–2** must be **prefixed** (or otherwise **explicitly tied**) to a **bracketed inject** (**`[Screen after action]`**, **`[Screen before action]`**, **`[Annotated after action]`**, **`[Zoom pointer after action]`**, …). **Do not** imply pixels without naming the **frame** they come from.

**Full completion** — Do not treat **subset** work (e.g. **4/10** items, half a form, truncated copy) or **repeat “done”** without **new** proof as finished; **`response`** must match verified scope.

**Overlay discipline** — Stages **1–4**: **no** overlay **`index`**, digits, or “bbox N”. **`index`** **only** inside **`Location:`**.

**Stepwise derivation (mandatory)** — Write **`thoughts`** like a **graded proof**: **each** stage (**`Pointer:`** … **`Location:`**) and **each** numbered line inside **`Pointer:`** or **`Location:`** may use **only** facts and conclusions **already shown earlier in that stage** (or in **prior** stages). **Do not** jump to a final verdict, tool choice, **`index`**, **`x`/`y`**, or **`hover`** **before** the line or stage that **earns** it. **Do not** skip intermediate substeps or collapse several steps into one sentence (e.g. no “**`Pointer:`** lines **`1`–`3`** plus **`4 Conclusion (Center-only rule)`** in one line” in real replies). If a step does **not** apply (e.g. **`Location:`** omitted), **do not** pretend it ran.

### Tool geometry: overlay **index** vs **coordinates** (computer profile)

Use **`[Annotated after action]`** overlay numbers **only** with **index-based** methods below. Use **`x`/`y`** (or drag endpoints) with **coordinate-based** methods; optional **`[CUR_SCREEN]`** prose may list **Pointer position** (capture pixels and session-normalized **x**/**y**) and **pointer neighbor reference bbox** entries — those are **anchors for coordinate calls only**, not targets for `index` clicks.

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

---

### 1) Pointer

**1. Goal (目标)** — Judge **geometry only** for the **synthetic mouse pointer hotspot** on **`[Screen after action]`** and zoom crops — **not** text **caret** / insertion bar (**caret is out of scope** for **`Pointer:`**). The prior step’s aim is the **center** of the control named on **`2 Intended aim on [Screen after action]:`** (the **geometric center** of that control on the post-action frame). **`accurate`** applies **only** when the hotspot **coincides** with that center (**Center-only rule**). Hotspots **outside** the control, **only nearby** (padding, gutter, margin beside the control), **inside the control’s bbox but on a border strip** (top / bottom / left / right **edge** or **corner**), or on the **wrong sub-part** are **`abnormal`**, not “close enough.” **`index`** / **`x`/`y`** clicks are defined to hit **center** — “**inside** the same field / row” without center coincidence is **not** **`accurate`**. **No** overlay **`index`** or digits in **`Pointer:`**. Prefer **`[Zoom pointer after action]`** to judge center coincidence.

**2. Logic (逻辑)** — Build **only** forward: **(a)** what changed vs **`[Screen before action]`** (or **`n/a`**), **(b)** name the **aim on `[Screen after action]`** the last step tried to hit (**always** state **aim = … center** in **line 2** when the target is a control), **(c)** cite **ordered**, **frame-tagged** facts that place the hotspot **relative to that center** (**`On [Screen after action]:`** first, then zooms — e.g. **on rim**, **below center**, **outside bbox to the left**), **(d)** **then** on the **`4 Conclusion (Center-only rule):`** line emit **one** **`accurate` \| `abnormal` \| `n/a`** per **Center-only rule**; reason **recombines (a)–(c)** only. **`4 Conclusion (Center-only rule)`** must **not** introduce observations absent from **line 3**. If the tool turn has **no** pointer geometry (e.g. **`clipboard:read`**), use the **n/a chain** in the **Mini examples**.

**3. Template (模板 — `Pointer:` chain + analysis flow)**

**`Pointer:` chain (lines `1`–`3` + `4 Conclusion (Center-only rule)`)** — Same **numbered-line discipline** as **`Next:`** and **`Location:`**.

**Pointer chain**

1. **View vs before** — **`unchanged`** \| **`changed — <short unindexed cue>`** \| **`n/a — no before frame`** when **`[Screen before action]`** is missing. Task-relevant UI only.

2. **Intended aim on `[Screen after action]`** — Name the **aim geometry** the last step **tried** to hit **as it would read on the current post-action full-screen truth** (**`[Screen after action]`**): band, label/shape, row identity — **traits only**; end with **aim = … center** (geometric center of that control, or stated axis center for bars/fields). **No** **`accurate` / `abnormal` / `n/a`** here; **no** digits.

3. **Evidence (hotspot vs aim)** — **First** **`[Screen after action]`**, **then** zooms as needed (prefer **`[Zoom pointer after action]`**): **where** the **pointer hotspot** sits **relative to the intended center** named in **Intended aim** (coincident vs offset: **which direction**, **on edge / corner**, **outside bbox**, **nearby only**). Use **separate prefixed clauses**, e.g. **`On [Screen after action]: …`** then **`On [Zoom pointer after action]: …`**. **Facts only** — **do not** state the **`Pointer:`** verdict on this line. **Do not** describe or judge **caret** here.

4. **Conclusion (Center-only rule)** — **`accurate`** \| **`abnormal`** \| **`n/a`** — **one** label, then **reason** that **only** restates **lines 1–3** (no new facts). The reason must **repeat** the **same** hotspot↔target **spatial layout** already given in **line 3** (e.g. left/right of center, on rim, outside bbox), using **control / region names** — **do not** substitute **`line 2` / `line 3`** for that description. **Do not** use a vague paraphrase alone (e.g. only “misplaced”). **On-wire prefix** must be the literal **`4 Conclusion (Center-only rule):`**.

**Strict derivation inside `Pointer:`** — **`4 Conclusion (Center-only rule)`** is **forbidden** until **lines 1–3** are written **in numeric order**. **Line 2** must **not** embed verdict labels; **line 2** must state the **center** aim when judging a control. **Line 3** holds **all** geometry facts referenced in **`4 Conclusion (Center-only rule)`**, with **frame tags** per **Image-grounded clauses**; **line 3** must locate the hotspot **relative to that center** for **Center-only rule**. **`4 Conclusion (Center-only rule)`** reasons must **name** the **same** widgets/regions as **line 3** — **not** “**`line 3`** says …” / “**`line 2`** aim …” as a substitute for those names.

**Center-only rule (for `accurate` vs `abnormal`)** — **`line 2`** names a control and **aim = its geometric center** (or the stated axis center, e.g. URL field **horizontal center**). **`accurate`** — hotspot **coincides** with that point: on **`[Zoom pointer after action]`** the tip/glyph **overlaps** the center (allow **only** minimal ambiguity from cursor art / scaling — **not** a visibly displaced hit on the **rim**, **peripheral band** of the bbox, or **padding** outside the silhouette). **`abnormal`** — any of: hotspot **outside** the control’s silhouette; **inside** but on **any edge or corner** of the bbox instead of center; **only adjacent** (beside, above, below) without center overlap; **wrong sub-part** (label vs icon, row text vs knob); **wrong row/panel**. Do **not** call **`abnormal`** placements **`accurate`** because they are “on the right widget” or “inside the control.”

**Conclusion labels** — **`accurate`** — **only** if **Center-only rule** passes (**center coincidence**). **`abnormal`** — hotspot placement fails **Center-only rule** (includes edge / nearby / outside / wrong sub-part). **`n/a`** — intended aim or **pointer** hotspot **not** visible on the injects.

**Required form**

```text
Pointer:
1 View vs before: <unchanged | changed — unindexed cue | n/a — no before frame — cite [Screen before action] vs [Screen after action] when both exist>.
2 Intended aim on [Screen after action]: <traits; aim = <control> geometric center (or axis center); no verdict words>.
3 Evidence (hotspot vs aim): <On [Screen after action]: … vs intended center/aim named in Intended aim>; <On [Zoom pointer after action] (optional): …> — pointer hotspot only; no caret.
4 Conclusion (Center-only rule): <accurate | abnormal | n/a> — <reason: name controls/regions + hotspot↔aim spatial relation already shown in Evidence; no new facts; no “line 2/3” shorthand; no vague “misplaced” alone>.
```

**Rules (short):** **1** = before/after delta only (**name injects** when comparing). **2** = aim on **`[Screen after action]`** — traits + **center** aim (**no** verdict). **3** = **ordered** **On [Screen after action]:** then zooms — hotspot **vs the named intended center** (**no** caret). **`4 Conclusion (Center-only rule)`** = **one** verdict + reason **from lines 1–3** only (**name** targets and layout, not **`line N`**); **`accurate`** **only** under **Center-only rule**.

**4. Mini examples (样例)**

**Progressive blocks** (line **1** only, **1–2**, **1–3**) each add **one** new numbered line for teaching; they are **not** complete **`Pointer:`** replies. **Full chains** always emit **`1` → `2` → `3` → `4 Conclusion (Center-only rule)`** in that order in real **`thoughts`** — **never** skip **2** or **3**, and **never** emit **`4 Conclusion (Center-only rule)`** before **3** (see **anti-patterns**).

**Mini example — line 1 only (View vs before)**

```text
Pointer:
1 View vs before: changed — On [Screen before action]: save dialog visible; On [Screen after action]: save dialog absent.
```

**Mini example — lines 1–2 (add intended aim)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same settings panel.
2 Intended aim on [Screen after action]: Bluetooth toggle knob on second settings row; aim = knob center.
```

**Mini example — lines 1–3 (add evidence; no verdict yet)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same API keys table.
2 Intended aim on [Screen after action]: copy icon immediately right of masked key cell; aim = icon center.
3 Evidence (hotspot vs aim): On [Screen after action]: pointer glyph sits on masked **key ciphertext** immediately **left** of the copy icon; **not** over **copy-icon disk center**. On [Zoom pointer after action]: hotspot offset from **copy-icon geometric center** vs **intended icon center**.
```

**Mini example — anti-patterns (`Pointer:` numeric order and **`4 Conclusion (Center-only rule)`** wording)**

**Forbidden — numeric order:** **do not** print **`4 Conclusion (Center-only rule)`** before **`3 Evidence`**.

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same settings panel.
2 Intended aim on [Screen after action]: Bluetooth toggle knob on second settings row; aim = knob center.
4 Conclusion (Center-only rule): abnormal — (forbidden when **Evidence** is missing above).
3 Evidence (hotspot vs aim): On [Screen after action]: … On [Zoom pointer after action]: …
```

**Forbidden — vague `4 Conclusion (Center-only rule)`:** it must **restate** the **same** hotspot↔aim **spatial relation** already shown in **line 3** (side, edge, gap, inside/outside bbox, vs center). A bare label like “misplaced” **without** that geometry is **invalid**.

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same settings panel.
2 Intended aim on [Screen after action]: Bluetooth toggle knob on second settings row; aim = knob center.
3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer glyph on second row **label text**, **left** of the toggle knob housing, **not** over knob disk center. On [Zoom pointer after action]: hotspot on label baseline **lateral left** of knob disk; visible **separation** from knob geometric center vs **intended knob-disk center**.
4 Conclusion (Center-only rule): abnormal — pointer hotspot misplaced.
```

**Correct — lines `1`→`2`→`3`→`4`; concrete `4 Conclusion (Center-only rule)` tied to Evidence:**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same settings panel.
2 Intended aim on [Screen after action]: Bluetooth toggle knob on second settings row; aim = knob center.
3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer glyph on second row **label text**, **left** of the toggle knob housing, **not** over knob disk center. On [Zoom pointer after action]: hotspot on label baseline **lateral left** of knob disk; visible **separation** from knob geometric center vs **intended knob-disk center**.
4 Conclusion (Center-only rule): abnormal — hotspot on **row label left of knob housing**, **lateral gap** to **Bluetooth knob-disk center** — **wrong sub-part** / **not center coincidence**.
```

**Mini example — full chain (`accurate`)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same dialog chrome.
2 Intended aim on [Screen after action]: blue Save pill in dialog footer; aim = pill center.
3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer rests on blue Save pill in footer with tip over **pill interior**, not on footer margin or pill rim. On [Zoom pointer after action]: hotspot overlaps Save pill **geometric center** within minimal cursor-width tolerance vs **intended Save pill center** — **not** on edge band.
4 Conclusion (Center-only rule): accurate — synthetic pointer overlaps **Save pill geometric center**.
```

**Mini example — full chain (`abnormal`)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same API keys table.
2 Intended aim on [Screen after action]: copy icon immediately right of masked key cell; aim = icon center.
3 Evidence (hotspot vs aim): On [Screen after action]: pointer glyph sits on masked **key ciphertext** immediately **left** of the copy icon; **not** over **copy-icon disk center**. On [Zoom pointer after action]: hotspot offset from **copy-icon geometric center** vs **intended icon center**.
4 Conclusion (Center-only rule): abnormal — hotspot on **masked key ciphertext**, not on **copy-icon geometric center** — **wrong sub-part**.
```

**Mini example — full chain (`abnormal`, inside bbox but on bottom rim — not center)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same dialog chrome.
2 Intended aim on [Screen after action]: blue Delete pill in footer; aim = pill geometric center.
3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer lies on blue Delete pill but **flush on bottom rim** of the pill, not the middle. On [Zoom pointer after action]: hotspot stays on lower **edge** of pill footprint vs **intended Delete pill geometric center**.
4 Conclusion (Center-only rule): abnormal — hotspot on **Delete pill bottom rim**, not **pill geometric center**.
```

**Mini example — full chain (`abnormal`, outside bbox — nearby only)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same toolbar.
2 Intended aim on [Screen after action]: star bookmark icon in omnibox strip; aim = icon center.
3 Evidence (hotspot vs aim): On [Screen after action]: pointer hotspot sits in **empty padding immediately left** of the star icon, **outside** the icon’s circular bbox, not overlapping icon center. On [Zoom pointer after action]: gap visible between hotspot and **star icon center** vs **intended bookmark center**.
4 Conclusion (Center-only rule): abnormal — hotspot in **padding left of star disk**, **outside star silhouette**, not **star / bookmark center**.
```

**Mini example — full chain (`n/a` before frame, `abnormal` off-center)**

```text
Pointer:
1 View vs before: n/a — no before frame for comparison.
2 Intended aim on [Screen after action]: omnibox URL field; aim = field horizontal center.
3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer visible on URL bar **left** edge (off center). On [Zoom pointer after action]: same left-edge placement, **left** of **intended omnibox URL field horizontal-center**.
4 Conclusion (Center-only rule): abnormal — pointer on **URL bar left edge**, not on **URL field horizontal-center**.
```

**Mini example — full chain (`n/a`, target gone)**

```text
Pointer:
1 View vs before: changed — On [Screen before action]: list with trash row; On [Screen after action]: full-window modal replaces list.
2 Intended aim on [Screen after action]: trash icon on prior list row; aim = icon center.
3 Evidence (hotspot vs aim): On [Screen after action]: prior list / trash icon region not visible; cannot place pointer hotspot against **intended trash icon center**.
4 Conclusion (Center-only rule): n/a — **trash row / trash icon** not visible on **[Screen after action]**; cannot compare hotspot to **trash icon center aim**.
```

**Mini example — full chain (`n/a`, pointer invisible)**

```text
Pointer:
1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same modal dialog chrome.
2 Intended aim on [Screen after action]: primary button in modal dialog; aim = button center.
3 Evidence (hotspot vs aim): On [Screen after action]: no visible synthetic **pointer** glyph over the dialog.
4 Conclusion (Center-only rule): n/a — no **synthetic pointer** over modal; cannot verify hotspot vs **primary button center aim**.
```

**Mini example — full chain (`n/a`, tool has no pointer geometry)**

```text
Pointer:
1 View vs before: n/a — this turn is clipboard:read only.
2 Intended aim on [Screen after action]: n/a — no on-screen click aim for clipboard:read.
3 Evidence (hotspot vs aim): On [Screen after action]: n/a — no pointer hotspot to compare for clipboard:read on this frame.
4 Conclusion (Center-only rule): n/a — Pointer stage not used for read-only clipboard step.
```

---

### 2) Verify

**Purpose:** Judge the **last automated step** using **Pointer** + before/after screens + grounded tool text.

**Step A — Visible evidence:** **concrete visible outcome** · **`no visible outcome`**.

**Step B — Task type:** Pick **exactly** one — **`deferred`** · **`non-deferred`** (always add a **short reason** on the **`Task type:`** line).

- **`deferred`** — Honest success or failure is **not** expected on **this** screenshot alone: open web page/download/upload/export/print queue/sync/background job; proof on **another surface** (Transfers, folder, progress URL); **or** off-screen evidence (**clipboard** after silent **Copy** with no toast/status). Same step can still be **`FAILED`** if the UI clearly shows the wrong action.

- **`non-deferred`** — The step expects an **immediate on-canvas** change: dialog open/close, toggle, focus, tab switch, inline validation, scroll/viewport motion, new row, submit error/success on this view, etc.

**Clipboard nuance:** Silent **Copy** → verifying the **payload** is **`deferred`** (proof off-screen, **`clipboard:read`**). A **wrong control / abnormal pointer** on the copy click is still **`FAILED`** under **rule 1**, independent of deferred vs non-deferred wording for the clipboard question.

**Step C — Outcome:** Apply **1→6** below. **`NFO`** is only this turn’s label for **rule 2**—**no** lock id, **no** carry-over field in **`tool_args`**.

1. Evidence **contradicts** intent, or **small-target** step with **`Pointer:`** **`abnormal`** → **`FAILED`**.  
2. **`no visible outcome`** + **`deferred`** + no strong wrong-operation cue → **`NFO`**. **Next** must **verify** first—**no** same **trigger** until a check shows pass/fail.  
3. **`no visible outcome`** + **`non-deferred`** → **`FAILED`**.  
4. **Concrete outcome** **fully** matches this step’s intent and (precision click) **`Pointer:`** **`accurate`** — i.e. **Center-only rule** satisfied on injects — → **`VERIFIED`**.  
5. **Concrete** visible progress **toward** this step’s intent, but the **full** outcome the step claimed is **not** yet satisfied on **`[Screen after action]`**; **no** contradiction with intent so far → **`PARTIAL`**.  
6. Else → **`FAILED`** or **`NFO`** (conservative).

**Four outcomes — definitions and separation**

- **`VERIFIED`** — Rule **4**: the **whole** step intent is satisfied on available evidence (and precision steps need **`Pointer:`** **`accurate`**, i.e. **Center-only rule** / hotspot on target **center**). Use when nothing material is left **for this step** on screen.
- **`PARTIAL`** — Rule **5**: you see **clear forward motion** (counts, lists, panes, partial lists) but the step’s **`goal`** implied **more** than what is visible now (e.g. “export **all**” shows **some** rows; “open settings → Accounts” only reached **General**). **Not** “no UI change”: that is **`no visible outcome`** → **`NFO`** or **`FAILED`**, not **`PARTIAL`**. **Not** wrong UI: that is **`FAILED`**. **`response`** must **not** treat the **overall** user task as finished while **`Verify:`** stays **`PARTIAL`** for the remaining scope (see **Full completion**).
- **`NFO`** — Rule **2** only: **`deferred`** + **`no visible outcome`** + no clear wrong-operation signal — **cannot** tell pass/fail **from this frame**; proof is **elsewhere** or **later**. **`NFO`** is **unverified**, not “half done on screen.”
- **`FAILED`** — Rules **1**, **3**, **6**, or visible proof that the step did **not** work / hit the wrong place.

**`NFO`** = need further verification (rule **2** only). Not “probably OK.”

**`PARTIAL` vs `NFO`:** **`PARTIAL`** needs **concrete** partial proof on **`[Screen after action]`**; **`NFO`** is for **no** honest on-canvas proof yet on a **deferred** path.

**`PARTIAL` vs `VERIFIED`:** **`VERIFIED`** means the step’s intent is **complete** for what this tool call promised; **`PARTIAL`** means **progress only**—**`Next:`** must continue until **`VERIFIED`** or **`FAILED`** for the remaining work.
**Required form**

```text
Verify:
Before vs after: <delta | same>.
Visible evidence: <concrete | no visible outcome> — <cue>.
Task type: <deferred | non-deferred> — <reason>.
Prior tool text (if any): <role only; no secrets>.
Pointer echo: <accurate | abnormal | n/a> — <must agree with **`Pointer:`** **`4 Conclusion (Center-only rule)`**; when stating hotspot placement, prefix On [Screen after action]: or other bracketed inject used in **`Pointer:`** **line 3**>.
Outcome: <VERIFIED | PARTIAL | NFO | FAILED> — <rule 1–6>.
```

**`Pointer echo`** — **One** line that **re-echoes** **`Pointer:`** **`4 Conclusion (Center-only rule)`** (label + reason). **Do not** contradict **`Pointer:`** **lines 1–3** or **`4 Conclusion (Center-only rule)`**; **do not** introduce **new** geometry not already on **`Pointer:`** **line 3**. When the echo mentions **where** the hotspot sits, use the **same** **bracketed frame** tags as **`Pointer:`** **line 3** (typically **`On [Screen after action]:`** first).

**Mini examples — Step C rules**

```text
Verify:
Before: export dialog. After: history dialog open instead. Concrete — wrong panel. non-deferred. Pointer echo: n/a — verdict driven by wrong panel, not hotspot geometry. FAILED — rule 1 contradicts.
```

```text
Verify:
After: same table row. Concrete — no new row. non-deferred — trash icon was target. Pointer echo: abnormal — On [Screen after action]: hotspot on row text left of trash icon vs icon-center aim. FAILED — rule 1 small-target miss.
```

```text
Verify:
Before: API keys table. After: same; no toast. no visible outcome. deferred — clipboard proof. Pointer echo: accurate — On [Screen after action]: hotspot on copy icon center. NFO — rule 2; next clipboard:read.
```

```text
Verify:
Before: form with errors. After: same; Submit still enabled; no new message. no visible outcome. non-deferred — expect submit result. Pointer echo: accurate — On [Screen after action]: hotspot on Submit pill center. FAILED — rule 3.
```

```text
Verify:
Before: empty search field. After: field shows typed query. Concrete — text visible. non-deferred. Pointer echo: accurate — On [Screen after action]: hotspot on field center. VERIFIED — rule 4.
```

```text
Verify:
Before: export dialog lists 10 files. After: progress “3 of 10 complete”; seven rows still pending. Concrete — partial batch. non-deferred — multi-file export. Pointer echo: accurate — On [Screen after action]: hotspot on Export control center. PARTIAL — rule 5; next wait or scroll list, then recount.
```

```text
Verify:
After: subtle spinner started; main canvas unchanged; goal was “export finished”. no visible outcome. deferred — check queue. Pointer echo: accurate — On [Screen after action]: hotspot on Export control center. NFO — rule 6 conservative.
```

```text
Verify:
After: same idle page; goal was “open sidebar”. no visible outcome. non-deferred. Pointer echo: n/a — On [Screen after action]: no new sidebar chrome vs intent. FAILED — rule 6 conservative.
```

**Mini examples — evidence / task type (Step A / B)**

```text
Verify:
Visible evidence: concrete — red banner text changed to a new error line. Task type: non-deferred — inline validation.
```

```text
Verify:
Visible evidence: no visible outcome — list tail unchanged after scroll attempt. Task type: non-deferred — viewport should move.
```

```text
Verify:
Task type: deferred — started upload; completion on progress URL or queue, not this frame.
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

2. **Target on `[Screen after action]`** — **One** **operationally clear** aim **on the current post-action full-screen inject only**: name the **control or row** you will use, with **visible** **label** (exact or partial text, or “unlabeled icon”), **shape** (pill, chip, row, tab, field, glyph), **color / emphasis** if it disambiguates, **band / region**, and **neighbors**. You may **prefix** sub-clauses with **`On [Screen after action]:`** for each facet group. Traits must be **already visible and uniquely describable** on **`[Screen after action]`**; **no** **`[Annotated after action]`** here. **Do not** use **relationship** nicknames as the **label** **unless** that **exact** string appears on the frame.

**No speculative or procedural text in line 2** — **Do not** use **modal** qualifiers (**“might”**, **“probably”**, **“could be labeled”**) or **multi-phase** hunt language (**“locate … then identify”**, **“find the right row first”**, **“need to pick among …”**) in **line 2**. If nothing is **yet** uniquely nameable on the frame, **line 2** must describe a **preparatory** visible target for **this** turn’s **`Tool kind`** (**scroll** surface, **expand** chevron, **wait** region, etc.), or **`Tool kind`** must be **inspect-only** (**`clipboard:read`**, **`wait`**) until a later turn can write a click line **2**. **`Tool kind`** is the **tool class only** — **no** search narrative there either.

**Branch (feeds line 1):** If **`Verify:`** was **`NFO`** → **line 1** must show that **`Next:`** will **inspect** (e.g. **`clipboard:read`**, queue / folder / **`wait`**)—**not** repeat the same trigger first. If **`Verify:`** was **`PARTIAL`** → **line 1** must show **continuation** toward the **remaining** scope—**not** claiming the **full** user task is done in **`response`** until a later turn **`VERIFIED`** that scope.

**Required form**

```text
Next:
1 Prior stages & sub-goal: Verify: <verdict + short reason>; Repetition: <OK | …>; this turn: <one concrete advance — no overlay digits>.
2 Target on [Screen after action]: <label or unlabeled icon; shape; color if needed; band/region; neighbors — visible on this frame only>.
Tool kind: <e.g. clipboard:read | mouse click — tool class only; still no digits; no search narrative>.
```

**Mini example — anti-pattern (line 2 must not read like a hunt)**

```text
Next:
1 Prior stages & sub-goal: Verify: VERIFIED; Repetition: OK; this turn: open the intended personal chat from the sidebar.
2 Target on [Screen after action]: locate a specific person’s thread — might be a personal chat; look for familiar avatar or name.
Tool kind: mouse click — need to identify the right row first.
```

**Mini example — tight line 2 when the row title is literally on-frame**

```text
Next:
1 Prior stages & sub-goal: Verify: VERIFIED; Repetition: OK; this turn: open one chat row by its on-screen title.
2 Target on [Screen after action]: sidebar chat row whose **visible title text** matches the on-frame spelling (example: “Alice”); shape list row with avatar + title; band left chat list; neighbors: under the search field if visible.
Tool kind: mouse click — no digits here.
```

**Mini examples — branch**

```text
Next:
1 Prior stages & sub-goal: Verify: NFO — silent copy needs off-screen proof; Repetition: OK; this turn: read system clipboard.
2 Target on [Screen after action]: no named on-screen control for read; region: current app surface after copy.
Tool kind: clipboard:read — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: FAILED — submit still visible; Repetition: OK; this turn: retry primary submit from the dialog.
2 Target on [Screen after action]: gray pill labeled “Submit” or unlabeled gray pill; shape pill; color gray; region modal dialog over dimmed app; neighbors: under red error banner, not Cancel text link beside it.
Tool kind: mouse click — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: NFO — download deferred; Repetition: OK; this turn: open browser Transfers surface.
2 Target on [Screen after action]: tab chip whose label includes “Transfers”; shape tab chip; region browser tab strip under omnibox; neighbors: among Download/History-style peers.
Tool kind: mouse click — no digits.
```

```text
Next:
1 Prior stages & sub-goal: Verify: VERIFIED last send; Repetition: OK; this turn: dismiss success snackbar.
2 Target on [Screen after action]: label “×” or short “Done” if visible; shape slim horizontal banner; color green emphasis; region top of page canvas; neighbors: below title/tabs strip, above main content.
Tool kind: mouse click — no digits.
```

**Mini example — full trait checklist on line 2 (before Target Locating)**

```text
Next:
1 Prior stages & sub-goal: Verify: FAILED — error banner + Submit still present; Repetition: OK; this turn: retry submit from modal.
2 Target on [Screen after action]: label “Submit” or unlabeled gray pill; shape pill; color gray; region modal dialog center stack; neighbors: under password fields, full column width — not Cancel text link.
Tool kind: mouse click — no digits.
```

---

### 5) Target Locating

Ground **`Next:`** **line 2** (the **`[Screen after action]`** target traits) — **do not** repeat **line 2** verbatim inside **`Location:`**; paraphrase overlay evidence there. **`Next:`** **line 1** (**Verify** / **Repetition** recap + sub-goal) is **context only** for the model — **do not** paste **line 1** into **`Location:`**. **`Location:`** **line 3** compares candidates to **`Next:`** **line 2** only. Follow the **target-locating chain** below **in order**; **do not** jump straight to “use **`index`** N”. Reference frames (pick the clearest for **the candidate overlay cell** and **bbox**): **`[Annotated after action]`** | **`[Zoom top after action]`** | **`[Zoom bottom after action]`** | **`[Zoom pointer after action]`**.

**Strict derivation inside `Location:`** — Same rule as **Stepwise derivation** in **Ground rules**, applied **per candidate**: **`Location:`** **line 2** must **not** reuse wording that **presupposes** **line 3**’s verdict (no early “this is / is not the **`Next:`** **line 2** target”); put accept/reject **only** on **line 3**. **Line 3** must show the **comparison** in **separate clauses** for **text**, **shape**, **color** (if used in **`Next:`** **line 2**), **band/position**, **neighbors** — then the **`match` \| `mismatch`** label (omit a facet clause **only** when **`Next:`** **line 2** truly omitted that facet). **Line 4** appears **only** after **`match`** on **line 3** and must **name** the counted controls that justify **`single`** vs **`multiple`**. **Line 5** appears **only** after **line 4** on a **`match`** path, or **once** as the exhausted **Outcome** when **no** **`match`**; it must **repeat** the routing implied by **lines 3–4** (no new facts).

**Target-locating chain (one candidate per pass; loop on mismatch)**

1. **Overlay `index` ↔ `bbox` (pair the candidate).** State the **frame**, then **candidate overlay `index` N** — **only the printed digit cell carries that `index`**; **`bbox` regions are not indexed** (no “`index` K **`bbox`**”). **Digit background:** the **candidate digit’s** **fill / background** color. **In `Location:` lines 1–2 and in any `bbox` / neighbor clause on line 3, do not write overlay numerals** (no neighbor “digit **K**”, no “printed **K**”, no repeating **N** beside traits) — use **bearing + stroke + layout names** only; the **`index` N** token at the **line 1** open (and **line 5** **`Outcome:`** repeating **`index` N** when the **index** route wins) is where the candidate numeral may appear. **Neighbor `bbox` sweep (relative to the candidate digit only):** along **above**, **below**, **left**, **right**, and **compound** directions when one neighbor tile touches two cardinals (**above-left**, **below-right**, …), list **each** flush‑abutting **`bbox`** using **bearing from the candidate** + that **`bbox` border stroke color**. **Same-color filter:** keep neighbor **`bbox`**es whose **stroke equals** the **candidate digit’s** **background**. **Nearest pair:** among those, choose the **closest** **`bbox`** to the **candidate** — **state direction + stroke colors**. If **no** same‑color neighbor on those rays, pair to the **single** **`bbox`** whose **stroke matches** the **candidate** and **bears** the **candidate** on its edge, after the sweep **rules out** other strokes. **One** paired **`bbox`**; reuse **`bbox`** on lines **1–4**.

**Mini example — line 1 only (`index` ↔ `bbox`)**

```text
Location:
1 [Annotated after action] index 11 — bbox pairing: **candidate digit** **background** green. Neighbors **relative to the candidate digit**: **above** none; **below** none; **left** abutting **bbox** stroke red; **right** none. Same-color (**green** stroke) among those neighbors: none. **Candidate digit** **flush** on **green**-stroke **bbox** around footer gear ⇒ paired **bbox** green; **left** red-border neighbor excluded by stroke mismatch. Anchor = footer icon row.
```

2. **Target traits inside that `bbox` (same frame as line 1)** — **Open** with **`On <exact same bracketed frame as line 1> —`** then describe what the **bbox** wraps (traits + **`inventory:`**). **Line 1** picks the **reference image**; **line 2** must **not** silently switch frames.

**Mini example — line 2 only (traits + inventory; same candidate as line 1)**

```text
Location:
1 [Annotated after action] index 11 — bbox pairing: **candidate digit** **background** green. Neighbors **relative to the candidate digit**: **above** none; **below** none; **left** abutting **bbox** stroke red; **right** none. Same-color (**green** stroke) among those neighbors: none. **Candidate digit** **flush** on **green**-stroke **bbox** around footer gear ⇒ paired **bbox** green; **left** red-border neighbor excluded by stroke mismatch. Anchor = footer icon row.
2 On [Annotated after action] — traits inside bbox: visible gray gear glyph; band footer; neighbors: **immediate left** red-border label strip (no gear glyph there); inventory: gear icon only.
```

3. **Target traits vs Next line 2 — verdict.** Compare **the same facets** **`Next:`** **line 2** used: **text**, **shape**, **color**, **screen position / band**, **neighbors**. **Mismatch if any required facet fails** (strict conjunctive check). **Match** means the **`Next:`** **line 2** target is **inside this `bbox`’s intended control** even when the **bbox** is “fat”; **do not** count inner widgets here — only whether **`Next:`** **line 2** and the described target align. Outcome: **`match`** | **`mismatch`**.

**Mini example — line 3 only (`mismatch`)**

```text
Location:
1 [Annotated after action] index 7 — bbox pairing: **candidate digit** **background** green. Neighbors **relative to the candidate digit**: **right** abutting **bbox** stroke gray; **left / above / below** none. Same-color among neighbors: none. **Candidate digit** on **left** edge of tall **green**-stroke **bbox** ⇒ paired **bbox** green; **right** gray-border neighbor excluded. Anchor = vertical rail/card stack.
2 On [Annotated after action] — traits inside bbox: rail + row clutter + cards; inventory: no footer gear glyph inside this **bbox**.
3 vs Next line 2: text: **Next** expects Settings gear glyph — **not** seen inside this **bbox**; shape: **Next** expects gear — **absent** in inventory; color: n/a; position/band: **Next** expects footer — gear sits **below** this stack on **[Screen after action]**, **outside** this **bbox**; neighbors: **Next** expects a slim strip **right** of **candidate** — gray strip visible there, gear still **absent** here; **any** miss ⇒ **mismatch**.
```

**Mini example — line 3 only (`match`)**

```text
Location:
1 [Annotated after action] index 11 — bbox pairing: **candidate digit** **background** green. Neighbors **relative to the candidate digit**: **above** none; **below** none; **left** abutting **bbox** stroke red; **right** none. Same-color (**green** stroke) among those neighbors: none. **Candidate digit** **flush** on **green**-stroke **bbox** around footer gear ⇒ paired **bbox** green; **left** red-border neighbor excluded by stroke mismatch. Anchor = footer icon row.
2 On [Annotated after action] — traits inside bbox: visible gray gear glyph; band footer; neighbors: **immediate left** red-border label strip (no gear glyph there); inventory: gear icon only.
3 vs Next line 2: text: n/a (icon target); shape: gear vs **Next** gear — OK; color: gray vs **Next** gray — OK; position/band: footer vs **Next** footer — OK; neighbors: **left** red-border strip vs **Next** — OK; all required facets OK ⇒ **match**.
```

4. **Exclusivity (only after `match` on line 3).** **Skip line 4** when line 3 is **`mismatch`.** After **`match`**, count **distinct hit targets** inside this **bbox**: **`single`** — effectively one actionable control for the **`Next:`** **line 2** aim — vs **`multiple`** — two or more separable controls (list them briefly). **Route:** **`single`** → prefer **`index` N`**; **`multiple`** → prefer **coordinates** (or another safe aim that isolates the **`Next:`** **line 2** control).

**Mini example — line 4 only (`single`)**

```text
Location:
1 [Zoom pointer after action] index 4 — bbox pairing: **candidate digit** **background** magenta. Neighbors **relative to the candidate digit**: **left** abutting **bbox** stroke cyan; **above / below / right** none. Same-color (**magenta**): only magenta **bbox** hugging ⋯ chip ⇒ paired **bbox** magenta **right** of row title; **left** cyan-border neighbor excluded.
2 On [Zoom pointer after action] — traits inside bbox: text ⋯, pill, row title band, **right** of title; inventory: ⋯ chip only.
3 vs Next line 2: text: ⋯ vs **Next** ⋯ — OK; shape: pill vs **Next** pill — OK; color: n/a; position/band: row title area vs **Next** — OK; neighbors: **right** of title vs **Next** — OK; all required facets OK ⇒ **match**.
4 Exclusivity: **single** — one ⋯ chip only; route: **index** path (line **3** **match** + one hit target).
```

**Mini example — line 4 only (`multiple`)**

```text
Location:
1 [Annotated after action] index 12 — bbox pairing: **candidate digit** **background** orange. Neighbors **relative to the candidate digit**: **left** **bbox** stroke green; **right** **bbox** stroke purple; **above** tab-strip **bbox** stroke gray; **below** none. Same-color (**orange**): wide toolbar **bbox** under **candidate** only — **nearest** orange stroke at **candidate** ⇒ paired **bbox** orange (omnibox strip); **left** green / **right** purple excluded.
2 On [Annotated after action] — traits inside bbox: URL string visible; text-input chrome; toolbar under tabs; **left** of star; inventory: URL field + star + extensions.
3 vs Next line 2: text: URL area vs **Next** URL field — OK; shape: text input vs **Next** field — OK; color: n/a; position/band: toolbar under tabs vs **Next** omnibox strip — OK; neighbors: **left** of star vs **Next** — OK; all required facets OK ⇒ **match**.
4 Exclusivity: **multiple** — URL field + star + extension icons as **separate** click targets; route: **coordinates** path (line **3** **match** + >1 target).
```

5. **Final outcome (one closing block per `Location:`).** If line 3 was **`match`**: run **line 4**, then **line 5** = **`index` N** | **coordinates** | **hover** (with why). If line 3 was **`mismatch`**: **stop at line 3** for that candidate — **no line 4**, **no line 5** for that candidate — and **open a fresh line-1 block** for another **`index`** (repeat **1–3** until **`match`**). **Only one** trailing **lines 4–5** pair per `Location:`: attach it to the **first matched** candidate; if **no** candidate **`match`**es after reasonable tries, emit **one** final **line 5**-style **Outcome** only (**coordinates**, **hover**, or tactic change per the rest of this doc) — **not** a per-candidate “retry” line **5**.

**Mini example — line 5 only (`index`, after `single` route)**

```text
Location:
1 [Zoom pointer after action] index 4 — bbox pairing: **candidate digit** **background** magenta. Neighbors **relative to the candidate digit**: **left** abutting **bbox** stroke cyan; **above / below / right** none. Same-color (**magenta**): only magenta **bbox** hugging ⋯ chip ⇒ paired **bbox** magenta **right** of row title; **left** cyan-border neighbor excluded.
2 On [Zoom pointer after action] — traits inside bbox: text ⋯, pill, row title band, **right** of title; inventory: ⋯ chip only.
3 vs Next line 2: text: ⋯ vs **Next** ⋯ — OK; shape: pill vs **Next** pill — OK; color: n/a; position/band: row title area vs **Next** — OK; neighbors: **right** of title vs **Next** — OK; all required facets OK ⇒ **match**.
4 Exclusivity: **single** — one ⋯ chip only; route: **index** path (line **3** **match** + one hit target).
5 Outcome: **index** 4
```

**Mini example — line 5 only (`coordinates`, after `multiple` route)**

```text
Location:
1 [Annotated after action] index 4 — bbox pairing: **candidate digit** **background** cyan. Neighbors **relative to the candidate digit**: **left** dialog-panel **bbox** stroke slate; **right** chrome-margin **bbox** stroke blue; **above / below** none flush. Same-color (**cyan**): single footer **bbox** spanning **OK**+**Cancel** under **candidate** ⇒ paired **bbox** cyan; **left** slate / **right** blue excluded.
2 On [Annotated after action] — traits inside bbox: “OK” label on blue pill; dialog footer band; **left** of Cancel pill; inventory: OK pill + Cancel pill.
3 vs Next line 2: text: “OK” vs **Next** OK — OK; shape: pill vs **Next** pill — OK; color: blue vs **Next** blue — OK; position/band: dialog footer vs **Next** footer — OK; neighbors: **left** of Cancel vs **Next** — OK; all required facets OK ⇒ **match**.
4 Exclusivity: **multiple** — OK pill and Cancel pill as **separate** targets in one **bbox**; route: **coordinates** path (line **3** **match** + >1 target).
5 Outcome: **coordinates** — OK pill center on **[Screen after action]** scale — no exclusive **click_index**
```

**Mini example — line 5 only (`coordinates`, wide toolbar candidate)**

```text
Location:
1 [Annotated after action] index 12 — bbox pairing: **candidate digit** **background** orange. Neighbors **relative to the candidate digit**: **left** **bbox** stroke green; **right** **bbox** stroke purple; **above** tab-strip **bbox** stroke gray; **below** none. Same-color (**orange**): wide toolbar **bbox** under **candidate** only — **nearest** orange stroke at **candidate** ⇒ paired **bbox** orange (omnibox strip); **left** green / **right** purple excluded.
2 On [Annotated after action] — traits inside bbox: URL string visible; text-input chrome; toolbar under tabs; **left** of star; inventory: URL field + star + extensions.
3 vs Next line 2: text: URL area vs **Next** URL field — OK; shape: text input vs **Next** field — OK; color: n/a; position/band: toolbar under tabs vs **Next** omnibox strip — OK; neighbors: **left** of star vs **Next** — OK; all required facets OK ⇒ **match**.
4 Exclusivity: **multiple** — URL field + star + extension icons as **separate** click targets; route: **coordinates** path (line **3** **match** + >1 target).
5 Outcome: **coordinates** — URL field center; not **index** 12
```

**Chaining — compose a full `Location:`** — After a **`mismatch`** **line 3**, **restart** at **line 1** for another **`index`** (repeat **lines 1–3** only for that candidate). After the **first** **`match`**, append **lines 4–5** **once** for that same candidate. If **no** **`match`**, end with **one** exhausted **line 5** built from **full** prior tries (**no** “**1–4:** summary” line that skips steps). **Exhausted** **`hover`** / **`coordinates`** / tactic **Outcome** still requires **every** tried candidate’s **lines 1–3** written out in full before that **single** **line 5** — the same rule as **Stepwise derivation**.

**Required form**

```text
Location:
1 <[Annotated after action] | [Zoom top after action] | [Zoom bottom after action] | [Zoom pointer after action]> index <N> — bbox pairing: **candidate digit** **background** <color>; neighbors **relative to the candidate digit** — **above / below / left / right / compound** each → <none | **bbox** stroke>; same-color neighbors → <list>; **nearest** same-color → paired **bbox** <direction + colors>; anchor <region>.
2 On <same frame as line 1> — traits inside bbox: <text; shape; color; band; neighbors by layout + strokes — no overlay numerals>; inventory: <what bbox wraps — for exclusivity only>.
3 vs Next line 2: <text: …>; <shape: …>; <color: … or n/a>; <position/band: …>; <neighbors: …>; <any miss ⇒ mismatch | all required facets OK ⇒ match>; <match | mismatch>.
4 Exclusivity (if match): <single | multiple — brief list>; route: <index path | coordinate path>.
5 Outcome: <index <N> | coordinates … | hover …> — after **match** + line **4**; **or** (no **match** after tries) **one** exhausted line: <coordinates … | hover … | tactic> with why.
```

**Rules (short):** **1** = **frame** + **`index` N** (sole overlay numeral on **line 1** open) + **candidate digit** **background** + neighbor **`bbox`**es by **direction from candidate** (**cardinal + compound**) + **stroke** (**no** other overlay numerals in **`bbox`** lines) + **same-color** filter + **nearest** paired **`bbox`** or candidate-on-matching-stroke fallback. **2** = **On the same inject as line 1** — traits **inside** that **bbox** (**no** line **3** verdict here). **3** = **facet-by-facet** compare to **`Next:`** **line 2** (see **Required form**), then **`match` \| `mismatch`**; **one failed facet ⇒ mismatch**. **4** = **only after match**; **single → index**, **multiple → coordinates**; **name** counted controls. **5** = **after match** + line **4**: final tool choice; **or** **once** at end of `Location:` when **every** candidate **`mismatch`**es / no safe **`index`** — **coordinates** / **hover** / tactic (**not** a line **5** per failed **`index`**); **no new facts** vs **lines 3–4**. **Chaining:** each new **`index`** restarts at **line 1**; multiple **`1`**…**3`** blocks in one `Location:` are **expected** until a **`match`** closes with **lines 4–5**.

---

### Full chain (complete JSON)

**Screen capture** is injected by the runtime, not a **`tool_name`** in your reply; the five-stage reasoning still lives in the **`thoughts`** string field.

**Screen-targeted tool** (includes **`Location:`**):

```json
{
  "thoughts": "Pointer:\n1 View vs before: unchanged — On [Screen before action] and On [Screen after action]: same error banner + Submit modal stack.\n2 Intended aim on [Screen after action]: gray Submit pill in modal dialog center stack; aim = pill center.\n3 Evidence (hotspot vs aim): On [Screen after action]: synthetic pointer rests on gray Submit pill in modal stack with tip over pill interior. On [Zoom pointer after action]: hotspot overlaps Submit pill geometric center within minimal cursor-width tolerance vs **intended Submit pill center** — not on rim.\n4 Conclusion (Center-only rule): accurate — synthetic pointer overlaps **Submit pill geometric center**.\n\nVerify:\nBefore: error banner. After: banner + Submit unchanged. no visible outcome for submit done. non-deferred. Pointer echo: accurate — On [Screen after action]: hotspot on Submit pill center. FAILED\n\nRepetition:\nLast rows differ; not flat 4×. OK\n\nNext:\n1 Prior stages & sub-goal: Verify: FAILED — error banner + Submit still present; no visible submit success; Repetition: OK; this turn: retry primary Submit from modal.\n2 Target on [Screen after action]: label \"Submit\" or unlabeled gray pill; shape pill; color gray; region modal dialog center stack; neighbors: under password fields — not Cancel text link.\nTool kind: mouse click — no digits here.\n\nLocation:\n1 [Annotated after action] index 6 — bbox pairing: **candidate digit** **background** green. Neighbors **relative to the candidate digit**: **left** form-field-column **bbox** stroke lime; **right** dimmed-app-margin **bbox** stroke blue; **above** modal-title **bbox** stroke gray; **below** none. Same-color (**green**): tall card **bbox** under **candidate** (**right** edge) vs lime column — **nearest** green stroke to **candidate** ⇒ paired **bbox** green tall card; **left** lime / **right** blue excluded by distance or stroke. Anchor = full form stack.\n2 On [Annotated after action] — traits inside bbox: visible gray pill labeled Submit under password fields; neighbor strip includes Cancel text link; inventory: email field, password fields, Submit pill.\n3 vs Next line 2: text: Submit label vs **Next** — OK; shape: pill vs **Next** pill — OK; color: gray vs **Next** gray — OK; position/band: modal center stack vs **Next** stack — OK; neighbors: under password fields, not Cancel vs **Next** — OK; all required facets OK ⇒ **match**.\n4 Exclusivity: **multiple** — email field + password fields + Submit pill as **separate** click targets; route: **coordinates** path (line **3** **match** + >1 target).\n5 Outcome: **coordinates** — Submit pill center; not **index** 6",
  "headline": "Retry submit via coordinates",
  "tool_name": "mouse:click_at",
  "tool_args": {}
}
```

**Omitting `Location:`** — whenever the method does **not** choose a new **`index`** or **`x`/`y`** on the capture (not only **`clipboard`**): e.g. **`wait`**, **`response`**, **`hotkey`**, **`mouse:…_current`**, **`move_offset`**, **`composite_action:type_text_at_focused`**, **`clipboard:read`** / **`clipboard:write`** — omit the whole **`Location:`** block inside **`thoughts`**. Example (clipboard read):

```json
{
  "thoughts": "Pointer:\n1 View vs before: n/a — this turn is clipboard:read only.\n2 Intended aim on [Screen after action]: n/a — no on-screen click aim for clipboard:read.\n3 Evidence (hotspot vs aim): On [Screen after action]: n/a — no pointer hotspot to compare for clipboard:read on this frame.\n4 Conclusion (Center-only rule): n/a — Pointer stage not used for read-only clipboard step.\n\nVerify:\nTool reply present. concrete for read step. non-deferred for tool. Pointer echo: n/a — agrees with Pointer 4 Conclusion (Center-only rule); no hotspot geometry for this read step. VERIFIED (read only).\n\nRepetition:\nOK\n\nNext:\n1 Prior stages & sub-goal: Verify: NFO — silent copy needs off-screen proof; Repetition: OK; this turn: read system clipboard.\n2 Target on [Screen after action]: no named on-screen control for read; region: current app surface after copy.\nTool kind: clipboard:read — no digits here.",
  "headline": "Read clipboard after silent copy",
  "tool_name": "clipboard:read",
  "tool_args": {}
}
```