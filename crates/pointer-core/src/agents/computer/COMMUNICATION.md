## On-wire shape: JSON object

Each desktop reply is **one** JSON object: string fields **`thoughts`**, **`headline`**, optional **`sidecar_tools`**, root **`tool_name`**, object **`tool_args`**.

**`thoughts`** holds the **six-stage block** below (**`Pointer:`** … **`Tool route:`**), in order.
**`Tool route:`** line **2** is the **only** place that picks the tool; it must match root **`tool_name`**.

Extended examples and anti-patterns: **`COMMUNICATION_FULL.md`** (not loaded at runtime).

---

## Global discipline (apply to every stage)

### A) Proof discipline — write like a graded math proof

1. Run stages **1 → 6** in order. **Do not** skip a stage.
2. Within each stage, write **numbered lines in order**. **Do not** emit a conclusion before the line that earns it.
3. A line may use **only** facts already shown **earlier in the same stage**, or conclusions from **prior** stages.
4. **Forbidden:** jumping to **`index`**, **`(x,y)`**, **`pass`/`fail`**, tool names, or **`therefore`** labels before the substeps that justify them.

### A2) Analysis before conclusion — on every line that decides

**Rule:** **Analysis first → conclusion last.** Same pattern as a math proof: write observations, then **`therefore`** / route label / verdict.

| Part | Content | Position |
|------|---------|----------|
| **Analysis** | **`On [Frame]:`** facts, T1–T3 pass/fail, **(a)(b)(c)**, placement, corner, offset — **no** final label | **First** — one or more clauses |
| **Conclusion** | **`therefore selected overlay index N`**, **`therefore adjacent reference index R`**, **`route: …`**, **`therefore (x,y)`**, **`Step result: …`** | **Last clause only** on that line |

**Forbidden on any line:**

- Open with **`therefore …`** or **`route: …`** then backfill analysis afterward.
- State **`selected overlay index 113`** before tracing which bbox digit 113 flush-adjacent to.
- Line **3** route before line **2** analysis + conclusion complete.
- **`Tool call`** before **`Location recap`** cites analysis from **Location**.

**Required pattern (Location line 2 example):**

```text
2 On [Annotated after action] — Analysis: T1 fail — no stroke on compose input; T2 fail — no digit on input;
   trace digit 113 → flush on blue neighbor list bbox; T3 fail — that bbox contains list row, not input;
   adjacent (a) blue list bbox; (b) digit 113 blue, flush on list bbox only;
   (c) list bbox wraps chat-list neighbor — does NOT wrap compose input.
   Conclusion: therefore adjacent reference index 113.
```

### B) Image discipline — every visual claim cites a frame

**Rule:** Any claim about pixels, layout, controls, pointer hotspot, or overlay digits must begin with **`On [Frame name]:`** naming an inject from the **current** **`[CUR_SCREEN]`** block.

**Forbidden:** describing UI from task text, memory, or guesswork without naming the frame you read.

**Overlay digits:** stages **1–4** — **no** overlay **`index`**, digits, or “bbox N”.
**`Location:`** line **1** — **no** overlay numerals (including in **`neighbors:`**).
Overlay **`index`** may appear **only** at the **end** of **`Location:`** line **2** and in **`Tool route:`** recap.

### C) Frame registry — which image each stage reads

| Frame | Present when | Read in stage | Must be used for |
|-------|--------------|---------------|------------------|
| **`[Screen before action]`** | prior turn exists | **Pointer** line **1**; **Verify** **Before vs after** (before side) | Pre-action layout; name intended aim |
| **`[Zoom pointer before action]`** | prior turn exists | **Pointer** line **2**; **Verify** **Mouse judgment** cite | Hotspot vs center (**4×**, ±50 px crop) — **required** when present |
| **`[Screen after action]`** | always | **Next** line **2**; **Location** line **1** placement; **Verify** **Before vs after** (after side) | Current full-screen layout |
| **`[Annotated after action]`** | always | **Location** lines **2–4**; coordinate geometry | Overlay digits, bbox strokes, **(x,y)** math |
| **`[Zoom top after action]`** | always | **Location** overlay frame pick | Top band — menu, title, tabs |
| **`[Zoom bottom after action]`** | always | **Location** overlay frame pick | Bottom band — dock, taskbar |
| **`[Zoom pointer after action]`** | always | **Location** overlay frame pick; pointer vicinity context | Near-pointer controls; small digits |

**First capture in thread:** omit **`[Screen before action]`** and **`[Zoom pointer before action]`** — **Pointer** uses **`[Screen after action]`** / **`[Zoom pointer after action]`** instead.

**`[CUR_SCREEN]`** also injects **Pointer position** and **Pointer neighbor reference bboxes** (session scale) — used in **Location** line **4** only.

---

## Pipeline (six stages)

| Stage | Prefix | Decides | Primary frame(s) |
|-------|--------|---------|------------------|
| 1 | **`Pointer:`** | Hotspot vs intended center for **last** action | **`[Zoom pointer before action]`** or after-action fallback |
| 2 | **`Verify:`** | Did **last** action succeed? **`Step result`** + **`Cause`** | **`[Screen before action]`** → **`[Screen after action]`** |
| 3 | **`Repetition:`** | Stuck loop? | **`[Recent desktop tool calls]`** text |
| 4 | **`Next:`** | **What** target this turn | **`[Screen after action]`** line **2** only |
| 5 | **`Location:`** | **Where** on overlay; route class | **`[Screen after action]`** then one overlay frame |
| 6 | **`Tool route:`** | **How** — one tool call | No new image reads — cite prior stages |

If **`Location:`** does not apply → **`Location: n/a`** — still run **`Tool route:`**.

---

## Tool geometry (index vs coordinates)

**Index methods** (overlay **`index`** from **current** annotated frame):  
**`mouse`:** `click_index`, `double_click_index`, `right_click_index`, `hover_index`, `drag_from_to_index` · **`composite_action`:** `type_text_at_index`, `scroll_at_index` · **`modified_click`:** `modified_click_index`.

**Coordinate methods** (**`x`/`y`** session scale, often 0–1000):  
**`mouse`:** `click_at`, `double_click_at`, `right_click_at`, `hover_at`, `drag_from_to_at` · **`composite_action`:** `type_text_at` · **`modified_click`:** `modified_click_at`.

**Neither:** `click_current`, `scroll_at_current`, `move_offset`, `type_text_at_focused`, **`hotkey`**, **`wait`**, **`clipboard:*`**, **`response`**.

Optional **`wait`** in **`tool_args`** (1–5 s) after successful mouse/hotkey/composite/modified_click — distinct from standalone **`wait`** tool.

### Off-frame tools (rare)

**`wait`**, **`response`**, **`clipboard:*`** — only when the stage chain already earned them. See tool prompts.

---

### 1) Pointer

**Goal:** Judge **geometry only** — synthetic pointer hotspot vs **intended control center**. **Not** before/after UI delta (**`Verify:`**). **Not** caret.

**Grounding:** Judge the **newest** row on **`[Recent desktop tool calls]`** — same action as **`Verify:`** **`Last automated action:`**. **Do not** invent prior actions.

#### Frame selection (before writing lines)

| Condition | Line 1 frame | Line 2 geometry frame |
|-----------|----------------|------------------------|
| **`[Screen before action]`** present | **`[Screen before action]`** | **`[Zoom pointer before action]`** — **required** |
| First capture (no before inject) | **`[Screen after action]`** | **`[Screen after action]`** and/or **`[Zoom pointer after action]`** |
| **`hotkey`**, **`wait`**, **`clipboard:*`**, etc. | name frame or **`n/a`** | **`n/a`** — non-pointer action |

#### Steps (strict order — do not skip)

| Step | Line | Action | Frame |
|------|------|--------|-------|
| P1 | **1 Intended aim** | Name control **that last action tried to hit**; end **`aim = … center`**. Traits only — **no** verdict. | Per table above |
| P2 | **2 Evidence (hotspot vs aim)** | **`On [geometry frame]:`** facts — hotspot position **relative to line 1 center**. End same line: **`Pointer on <aim>? yes.`** or **`no.`** | Geometry frame |
| P3 | **3 Conclusion (Center-only rule)** | Restate line **2** yes/no verbatim → **`therefore accurate`** or **`abnormal`**. Or **`n/a`** non-pointer. | — |

**Center-only rule:** **`yes`** ⇔ **`accurate`**. **`no`** ⇔ **`abnormal`**. Rim / wrong sub-part / parent region only ⇒ **`no`**.

#### Output template

```text
Pointer:
1 Intended aim on [Screen before action | Screen after action]:
   <newest [Recent desktop tool calls] row; precision click → traits + aim = center; else n/a>.
2 Evidence (hotspot vs aim):
   On [Zoom pointer before action | Screen after action | Zoom pointer after action]: <hotspot vs line 1 center>.
   Pointer on <aim from line 1>? yes. | no.
3 Conclusion (Center-only rule):
   Pointer on <aim>? yes | no. — therefore accurate | abnormal — <optional restate>
   | n/a — <non-pointer>.
```

#### Example (accurate)

```text
Pointer:
1 Intended aim on [Screen before action]: blue Save pill in dialog footer; aim = pill center.
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: hotspot over Save pill geometric center. Pointer on Save pill center? yes.
3 Conclusion (Center-only rule): Pointer on Save pill center? yes. — therefore accurate — hotspot on Save pill center on [Zoom pointer before action].
```

---

### 2) Verify

**Goal:** Judge **last automated action** using **Pointer** + **Before vs after** screens + grounded tool text.

**Fixed reminder** — immediately after **`Verify:`**, before **`Last automated action:`**:

`Indices reset each screen — no stale overlay index.`

#### Frame selection

| Field | Frame(s) | Rule |
|-------|----------|------|
| **Before vs after** | **`[Screen before action]`** → **`[Screen after action]`** | **Only** line that re-reads screenshots for UI delta. **Name both frames.** |
| **Mouse judgment** | Cite **`On [Zoom pointer before action]:`** when present | Must agree with **Pointer** line **3** |
| **Clear evidence** | **No** new frame read | Restate **Before vs after** outcome only |

First capture: **`Before vs after: n/a — no [Screen before action]`**.

#### Steps (strict order — do not skip)

| Step | Field | Source |
|------|-------|--------|
| V0 | Fixed reminder | exact line above |
| V1 | **Last automated action** | Newest **`[Recent desktop tool calls]`** row, or **`none — no prior desktop tool in this thread`** |
| V2 | **Before vs after** | Read **`[Screen before action]`** → **`[Screen after action]`** — UI delta only |
| V3 | **Clear evidence** | Label + restate V2 — **`supporting_evidence`** · **`contradicting_evidence`** · **`no_clear_evidence`** |
| V4 | **Action type** | **`deferred`** · **`non-deferred`** |
| V5 | **Mouse judgment** | **`non_mouse`** · **`mouse_miss`** · **`mouse_accurate`** — must match **Pointer** |
| V6 | **Lookup** | Copy keys from V3–V5 |
| V7 | **Match** | One row from table below |
| V8 | **Step result** + **Cause** | **Must equal Match** — **forbidden** before V6–V7 |

**First turn:** V1 = none → **`Lookup: n/a`** → **`Step result: n/a`** — omit **`Cause:`**.

#### Clear evidence

| Value | When |
|-------|------|
| **`supporting_evidence`** | Before vs after shows visible change **matching** intent |
| **`contradicting_evidence`** | Visible change **contradicts** intent |
| **`no_clear_evidence`** | No visible outcome for what action should have changed |

#### Action type

- **`deferred`** — pass/fail not settled on this screenshot (download/export/save-to-disk/queue).
- **`non-deferred`** — expect immediate on-canvas change.

#### Mouse judgment

| Value | When |
|-------|------|
| **`non_mouse`** | No precision click geometry — **`Pointer:`** **`n/a`** |
| **`mouse_miss`** | Precision click + **`Pointer:`** **`abnormal`** |
| **`mouse_accurate`** | Precision click + **`Pointer:`** **`accurate`** |

#### Verify → Step result (lookup table)

**`either`** = **`deferred`** or **`non-deferred`**.

| Clear evidence | Action type | Mouse judgment | Step result | Cause |
|----------------|-------------|----------------|-------------|-------|
| contradicting_evidence | either | mouse_miss | fail | precision_miss |
| contradicting_evidence | either | mouse_accurate | fail | wrong_operation |
| contradicting_evidence | either | non_mouse | fail | wrong_operation |
| supporting_evidence | either | mouse_miss | fail | precision_miss |
| supporting_evidence | either | mouse_accurate | pass | — |
| supporting_evidence | either | non_mouse | pass | — |
| no_clear_evidence | non-deferred | mouse_miss | fail | precision_miss |
| no_clear_evidence | non-deferred | mouse_accurate | fail | no_immediate_feedback |
| no_clear_evidence | non-deferred | non_mouse | fail | no_immediate_feedback |
| no_clear_evidence | deferred | mouse_miss | fail | precision_miss |
| no_clear_evidence | deferred | mouse_accurate | pending | off_frame_unverified |
| no_clear_evidence | deferred | non_mouse | pending | off_frame_unverified |

#### Output template

```text
Verify:
Indices reset each screen — no stale overlay index.
Last automated action: <newest row | none — no prior desktop tool in this thread>.
Before vs after: On [Screen before action]: … On [Screen after action]: … | n/a — no [Screen before action].
Clear evidence: <label> — restates Before vs after: <same UI words; no new On [Screen …]:>.
Action type: <deferred | non-deferred> — <reason>.
Mouse judgment: <non_mouse | mouse_miss | mouse_accurate> — <On [Zoom pointer before action]: when present>.
Lookup: Clear evidence=<same>, Action type=<same>, Mouse judgment=<same>; | n/a — no prior action.
Match: row <keys> → <Step result>, <Cause>; | row outside table → Step result n/a.
Step result: <pass | fail | pending | n/a>.
Cause: <only when Match says so; omit on pass>.
```

---

### 3) Repetition

**Goal:** Detect stuck loops from **`[Recent desktop tool calls]`** (oldest → newest).

```text
Repetition:
Rows: <same goal|action × N | differ>.
Screen: <flat | advanced>.
Verdict: <OK | STUCK> — <if STUCK: one tactic hint, no digits>.
```

**>3** consecutive same goal+action with no UI gain → **`STUCK`**.

---

### 4) Next

**Goal:** **What** to do this turn. **No** tools, **no** overlay digits, **no** coordinates.

**Prerequisite:** Finish **Verify** through **Match** first.

#### Frame selection

| Line | Frame | Rule |
|------|-------|------|
| **1** | — | Echo **Verify** conclusion, then **Lookup → Match → this turn** |
| **2** | **`[Screen after action]`** only | Target traits **visible on this frame** — **forbidden** **`[Annotated after action]`** here |

#### Steps — line 1 (strict sub-clause order)

| # | Label | Content |
|---|-------|---------|
| 1 | **`Verify:`** | Echo **`Step result`** + **`Cause`** from stage 2 |
| 2 | **`Repetition:`** | Echo verdict |
| 3 | **`Lookup:`** | **`Step result=`** + **`Cause=`** from Verify echo |
| 4 | **`Match:`** | One **Verify → Next** row |
| 5 | **`this turn:`** | Concrete next step from Match row |

#### Verify → Next

| Step result | Cause | this turn must… | Line 2 target |
|-------------|-------|-----------------|---------------|
| pass | — | Advance next sub-goal | **New** control on **`[Screen after action]`** |
| fail | wrong_operation | **Pivot** — different surface | **Different** control than failed action |
| fail | precision_miss | **Re-aim** same sub-target | **Same** control, tighter center wording |
| fail | no_immediate_feedback | **Retry** same on-canvas intent | **Same** control/region |
| pending | off_frame_unverified | **Wait or inspect** off-frame | **`n/a`** or visible shell |
| n/a | — | Open task from user goal (first turn) | First visible control on **`[Screen after action]`** |

#### Output template

```text
Next:
1 Prior stages & sub-goal:
   Verify: <Step result> — <Cause when present>;
   Repetition: <OK | STUCK>;
   Lookup: Step result=<same>, Cause=<same or —>;
   Match: row <Step result + Cause> → <this turn must…>;
   this turn: <concrete UI step, no overlay digits>.
2 Target on [Screen after action]: <label, shape, color, band, neighbors — visible on this frame only>.
```

---

### 5) Location

**Goal:** Turn **Next** line **2** into overlay analysis — frame, route class, optional **(x,y)**. **No** tool names.

**Input:** **Next** line **2** only (paraphrase — do not paste verbatim).

#### Routing table (read first — pick one path)

| Path | Condition | Line 2 output | Line 3 route | Line 4 |
|------|-----------|---------------|--------------|--------|
| **L0** | Non-overlay turn | — | — | **`Location: n/a`** |
| **L1** | Marked target, **`wrap count = 1`** | **`therefore selected overlay index N`** | **index path** | omit |
| **L2** | Marked target, **`wrap count > 1`** | **`therefore selected overlay index N`** | **coordinate path** | §5.4 |
| **L3** | **Unmarked** — no bbox+label on target | **`therefore adjacent reference index R`** | **coordinate path** | §5.4 |

#### Marked vs unmarked — decision gate (run on overlay frame **before** line 2)

**Question:** Does the **intended sub-target** (from **Next** line **2**) have its **own** legal overlay **bbox+label** pair on the chosen frame?

Read **`On [overlay frame]:`** — inspect **only** the intended control, not the whole panel.

**Three tests — all must pass for MARKED:**

| Test | Marked = YES when… | Unmarked = YES when… |
|------|-------------------|----------------------|
| **T1 Stroke** | A colored **bbox border** wraps the intended sub-target (or hugs it tightly as the anchor) | Intended sub-target is **visible** but **no** colored stroke wraps **that control** |
| **T2 Digit** | An overlay digit sits **flush** on **that** bbox border; digit **background** = border color | **No** digit pairs with the intended sub-target itself |
| **T3 Wraps target** | The bbox **paired with the digit** **geometrically contains** the intended sub-target (you can point to target **inside** that stroke) | Digit sits on a **neighbor** bbox (list row, sidebar, header, sibling button); target is **outside** that stroke — digit labels **neighbor only** |

**Decision (after T1–T3 analysis written — not before):**

| T1 + T2 + T3 analysis | Path | Line 2 conclusion (last clause only) |
|-----------------------|------|--------------------------------------|
| **All pass** | **L1** or **L2** (marked) | **`therefore selected overlay index N`** |
| **Any fail** | **L3** (unmarked) | **`therefore adjacent reference index R`** |

**Meaning of index:** **N** = digit labels **target's own** bbox. **R** = digit labels **neighbor** bbox only (anchor).

**Critical — digit labels its flush-adjacent bbox only (not “nearby target”):**

Before **(a)(b)(c)**, trace on the overlay frame: **which bbox stroke does the digit touch?** That bbox is what the digit **N** or **R** refers to — **not** a visually nearby control.

**T3 is decisive:** Does **that** paired bbox **geometrically contain** the intended sub-target’s clickable area (center you will act on)?

| Situation | Correct path | Wrong path |
|-----------|--------------|------------|
| Digit **113** flush on **left chat-list / contact-row** bbox; intended sub-target = **bottom message input** — input has **no** own digit | **Unmarked L3** → **`adjacent R=113`**; **(c)** wraps **list row / neighbor panel**, **not** input | ~~**selected index N=113**~~ — ~~**(c) wraps input**~~ when 113 labels **neighbor** bbox |
| Digit flush on bbox that **actually contains** input field; **(c)** input inside that same bbox | **Marked** → **`index N`** | — |
| Bbox wraps input **+** send button; digit on shared bbox; intended = input | **Marked L2** → **`index N`** + coordinate path | ~~unmarked~~ |
| **×** glyph; digit only on title-bar bbox left of **×** | **Unmarked L3** → **`adjacent R`** | ~~selected index N for ×~~ |
| Pick unmarked because need coordinates | Route follows **T1–T3 facts**, not tool preference | ~~unmarked because click_at~~ |

**Correct — unmarked: message input; digit 113 on neighbor region:**

```text
2 On [Annotated after action] —
   Analysis: intended sub-target = compose input (bottom bar);
   T1 fail — On input: no stroke wraps input alone;
   T2 fail — no digit flush on input bbox;
   trace digit 113 — flush on blue border of chat-list column left/above input;
   T3 fail — blue bbox paired with 113 contains list row, NOT compose input;
   adjacent (a) blue list bbox; (b) digit 113 blue, flush on list bbox only;
   (c) list bbox wraps chat-list neighbor — does NOT wrap compose input.
   Conclusion: therefore adjacent reference index 113.
3 Analysis: unmarked target (line 2 conclusion). Conclusion: route: coordinate path.
4 Analysis: On [Annotated after action]: input center right-below bbox 113 corner … | geometry deferred.
   Conclusion: therefore (x,y) ≈ … OR geometry deferred.
```

**Correct — marked: digit on bbox that truly contains input:**

```text
2 On [Annotated after action] —
   Analysis: T1 pass — green stroke wraps input; T2 pass — digit 42 flush on green border;
   T3 pass — green bbox contains input; (a) green border; (b) digit 42 flush; (c) wraps input.
   Conclusion: therefore selected overlay index 42.
3 Analysis: from line 1 traits wrap count = 1. Conclusion: route: index path.
```

**Anti-pattern — conclusion before analysis (forbidden):**

```text
2 therefore selected overlay index 113. (a) blue border; (b) digit 113 …
(forbidden — open with therefore / index before Analysis + T1–T3 + (a)(b)(c))
```

**Anti-pattern — neighbor digit mislabeled as target index (forbidden):**

```text
2 … Conclusion: therefore selected overlay index 113.
   Analysis: (c) wraps input — (forbidden order; and forbidden when 113 labels neighbor bbox)
```

**Contrast (unmarked — × button, title bar has index 12):**

```text
On [Annotated after action]: intended sub-target = × dismiss glyph.
T1: no stroke wraps × — fail.
→ Unmarked. Adjacent: title-bar bbox with digit 12.
(a)(b)(c) for neighbor title bbox → therefore adjacent reference index 12.
(c) wraps title strip, not ×.
```

**Order:** F1 → F2 → **M0: write T1–T3 Analysis** → line 1 → line 2 **Analysis then Conclusion** → line 3 **Analysis then Conclusion** → line 4 **Analysis then Conclusion**.

#### Frame selection (strict — two-step)

**Step F1 — placement (Location line 1 opening):**  
Read **`On [Screen after action]:`** — band / region / neighbors from **Next** line **2**.

**Step F2 — overlay analysis frame:** From F1 bearing, pick **one** frame:

| Bearing on **`[Screen after action]`** | Analyze on |
|----------------------------------------|------------|
| **Top** — menu, title, tabs | **`[Zoom top after action]`** if digits crowded; else **`[Annotated after action]`** |
| **Bottom** — dock, taskbar | **`[Zoom bottom after action]`** |
| **Near synthetic pointer** | **`[Zoom pointer after action]`** |
| **Central / wide** — dialog, toolbar | **`[Annotated after action]`** |

Write **`→ therefore analyze on [chosen frame]`** before any bbox / digit work.

**Step F3 — coordinate geometry (line 4 only):** Always on **`[Annotated after action]`** — **not** on zoom crops.

#### Steps — all paths (strict order)

| Step | Line | Action | Frame |
|------|------|--------|-------|
| L-F1 | **1** (start) | **`On [Screen after action]:`** placement → **`therefore analyze on [F2 frame]`** | Screen after → overlay |
| L-F2 | **1** (continue) | Target traits on overlay frame; **no** overlay digits on line **1** | F2 frame |
| L-M0 | **gate** | **Analysis:** T1–T3 pass/fail on intended sub-target — **then** pick L1/L2/L3 | F2 frame |
| L-M1 | **1** (marked) | **`traits inside that bbox:`** (analysis only — no route yet) | F2 frame |
| L-U1 | **1** (unmarked) | **`unmarked target — no overlay bbox+label`**; adjacent neighbors (layout names only) | F2 frame |
| L-2 | **2** | **Analysis:** T1–T3 + **(a)(b)(c)** → **Conclusion:** **`therefore index N`** or **`therefore adjacent R`** | Same F2 frame |
| L-3 | **3** | **Analysis:** cite line 1 **wrap count** or line 2 unmarked → **Conclusion:** **`route: …`** | — |
| L-4 | **4** | **Analysis:** placement, corner, offset on **`[Annotated after action]`** → **Conclusion:** **`therefore (x,y)`** or **geometry deferred** | **`[Annotated after action]`** |

#### Line 2 — Analysis then Conclusion

**Structure (mandatory order on one line):**

```text
2 On <overlay frame> — Analysis: <T1–T3 pass/fail; trace digit → which bbox; (a)(b)(c) facts>.
   Conclusion: therefore selected overlay index <N> | therefore adjacent reference index <R>.
```

**Marked (L1/L2):** Analysis must show T1–T3 **pass** and **(c)** target **inside** digit's bbox → Conclusion **`therefore selected overlay index N`**.

**Unmarked (L3):** Analysis must show T1–T3 **fail** on target, then **(a)(b)(c)** for **neighbor** only → Conclusion **`therefore adjacent reference index R`**.

On the **same** overlay frame as line **1**:

- **(a)** Restate wrapping bbox **border color** (marked) or neighbor bbox color (unmarked adjacent).
- **(b)** Digit **background** = that border color; digit **only** flush-adjacent to **that** bbox.
- **(c)** Bbox wraps **intended sub-target** (marked) or wraps neighbor, **not** unmarked target (unmarked).
- **Pass** → write Analysis clauses first → **Conclusion:** **`therefore selected overlay index N`** (marked) or **`therefore adjacent reference index R`** (unmarked) — **last clause only**.
- **Fail** → **`discard trial`** — retry; do **not** emit Conclusion.

#### Line 3 — Analysis then Conclusion

```text
3 Analysis: from line 1 traits wrap count = … | line 2 unmarked target.
   Conclusion: route: index path | coordinate path.
```

**Forbidden:** **`route: coordinate path`** before line **2** Conclusion is written.

#### Line 4 — Analysis then Conclusion (L2 / L3 only)

| Anchor (**N** or **R**) in **Pointer neighbor reference bboxes**? | Line 4 | Tool route (stage 6) |
|-------------------------------------------------------------------|--------|----------------------|
| **Yes** | placement → corner → **(xc,yc)** from reference → offset → **`therefore (x,y)`** | **`click_at`** / **`type_text_at`** at **(x,y)** |
| **No** | **`geometry deferred`** — no **(x,y)** | **`hover_index(anchor)`** this turn only |

**Coordinate geometry — Analysis clauses then Conclusion (last):**

```text
4 Analysis: On [Annotated after action]: sub-target vs anchor bbox …; corner …; (xc,yc) from reference …; offset Δx, Δy …
   Conclusion: therefore (x,y) ≈ (xc ± Δx, yc ± Δy) | geometry deferred.
```

#### Templates (pick one path)

**L0 — non-overlay**

```text
Location:
n/a — no overlay analysis this turn (<hotkey | wait | …>).
```

**L1 — marked, single / index path**

```text
Location:
1 Placement→frame: On [Screen after action]: <bearing> → therefore analyze on [<overlay frame>].
   <overlay frame> — target: …; wrapping bbox: <stroke + anchor>;
   traits inside that bbox: distinct controls = …; wrap count = 1; intended sub-target = ….
2 On <overlay frame> — Analysis: T1… T2… trace digit… (a)… (b)… (c)….
   Conclusion: therefore selected overlay index <N>.
3 Analysis: from line 1 traits wrap count = 1. Conclusion: route: index path.
```

**L2 — marked, multiple / coordinate path**

```text
Location:
1 … traits: wrap count > 1; intended sub-target = ….
2 … Analysis: … Conclusion: therefore selected overlay index <N>.
3 Analysis: from line 1 traits wrap count > 1. Conclusion: route: coordinate path.
4 Analysis: On [Annotated after action]: … Conclusion: therefore (x,y) | geometry deferred.
```

**L3 — unmarked / coordinate path**

```text
Location:
1 Placement→frame: On [Screen after action]: <bearing> → therefore analyze on [<overlay frame>].
   <overlay frame> — target: …; unmarked target — no overlay bbox+label; adjacent neighbors: ….
2 On <overlay frame> — Analysis: T1 fail… trace digit… adjacent (a)(b)(c)… neighbor wraps … not target.
   Conclusion: therefore adjacent reference index <R>.
3 Analysis: line 2 unmarked. Conclusion: route: coordinate path.
4 Analysis: On [Annotated after action]: offset from bbox <R> … Conclusion: therefore (x,y) | geometry deferred.
```

#### Invariants (Location)

- **INV-L0:** Every deciding line — **Analysis clauses first**, **`Conclusion:` / `therefore` last** — never open with conclusion.
- **INV-L1:** Line **1** — no overlay numerals.  
- **INV-L2:** Line **2** — **`index`** only as final token.  
- **INV-L3:** Line **4** — only on **`[Annotated after action]`**; anchor must match line **2**.  
- **INV-L4:** Unmarked → always coordinate path; never **`click_index`** on unmarked target.  
- **INV-L5:** geometry deferred → no **(x,y)**; no **`click_at`** same turn.
- **INV-L6:** **`therefore selected overlay index N`** only when the digit’s **flush-adjacent bbox** **contains** the intended sub-target (T1–T3 pass). **`therefore adjacent reference index R`** when the digit labels a **neighbor** bbox — **forbidden** **(c) wraps intended sub-target** if target is **outside** that bbox.

#### Example — L2 coordinate path

```text
Location:
1 Placement→frame: On [Screen after action]: OK/Cancel in central modal footer → therefore analyze on [Annotated after action].
   [Annotated after action] — target: OK pill; wrapping bbox: cyan-stroke footer OK+Cancel;
   traits: distinct controls = OK pill + Cancel pill; wrap count = 2; intended sub-target = OK pill.
2 On [Annotated after action] — Analysis: (a) cyan footer bbox; (b) cyan digit flush; (c) wraps OK pill.
   Conclusion: therefore selected overlay index 4.
3 Analysis: from line 1 traits wrap count = 2. Conclusion: route: coordinate path.
4 Analysis: On [Annotated after action]: OK toward left of bbox 4; corner bottom-left; (xc,yc) from reference for 4; offset right+up.
   Conclusion: therefore (x,y) ≈ (xc+Δx, yc-Δy).
```

---

### 6) Tool route

**Goal:** Commit **one** **`tool_name`** / **`tool_args`**. **Do not** re-analyze images.

**Prerequisite:** **Next** + **Location** (or **`n/a`**) complete.

#### Execution table (from Location recap)

| Location line 3 | Location line 4 | Tool route line 2 |
|-----------------|-------------------|-------------------|
| **index path** | — | **`mouse:click_index(N)`** etc. |
| **coordinate path** | **`therefore (x,y)`** complete | **`mouse:click_at(x,y)`** etc. |
| **coordinate path** | **geometry deferred** | **`mouse:hover_index(anchor)`** — forbid **`click_at`** |
| **n/a** | — | **`hotkey`** / **`wait`** / **`scroll_at_current`** / … |

**Action kind from Next `this turn:`:**

- Press icon/button/toggle **this turn** → **`mouse:click_*`** — not **`type_text_at_*`**
- Type/replace text **this turn** → **`composite_action:type_text_at_*`**

#### Steps

| Step | Line | Content |
|------|------|---------|
| T1 | **1** | **`Next recap: this turn:`** — copy **Next** line **1** **`this turn:`** clause only |
| T2 | **1** | **`Location recap:`** — route, anchor, **(x,y)** or deferred |
| T3 | **2** | **`Tool call this turn:`** — must match T1, T2, and tool prompt |

Root JSON **`tool_name`** = line **2** method.

#### Output template

```text
Tool route:
1 Next recap & Location:
   Next recap: this turn: <same as Next line 1 this turn: clause>;
   Location recap: <route; anchor; (x,y) or deferred | n/a>.
2 Tool call this turn: <tool_name:method> — <args per tool prompt>.
```

---

### Full chain (one JSON example)

```json
{
  "thoughts": "Pointer:\n1 Intended aim on [Screen before action]: blue OK pill in modal footer; aim = pill center.\n2 Evidence (hotspot vs aim): On [Zoom pointer before action]: hotspot on OK pill center. Pointer on OK pill center? yes.\n3 Conclusion (Center-only rule): Pointer on OK pill center? yes. — therefore accurate.\n\nVerify:\nIndices reset each screen — no stale overlay index.\nLast automated action: 2. mouse:click_index — footer region.\nBefore vs after: On [Screen before action]: dialog open. On [Screen after action]: same; dialog still open.\nClear evidence: no_clear_evidence — restates Before vs after: confirm not completed.\nAction type: non-deferred.\nMouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on OK center.\nLookup: Clear evidence=no_clear_evidence, Action type=non-deferred, Mouse judgment=mouse_accurate;\nMatch: row no_clear_evidence + non-deferred + mouse_accurate → fail, no_immediate_feedback;\nStep result: fail. Cause: no_immediate_feedback.\n\nRepetition:\nRows: differ. Screen: flat. Verdict: OK\n\nNext:\n1 Prior stages & sub-goal: Verify: fail — no_immediate_feedback; Repetition: OK; Lookup: Step result=fail, Cause=no_immediate_feedback; Match: row fail + no_immediate_feedback → Retry same on-canvas intent; this turn: confirm dialog via OK pill.\n2 Target on [Screen after action]: blue OK pill; shape pill; band modal footer; neighbors: Cancel pill right.\n\nLocation:\n1 Placement→frame: On [Screen after action]: OK in central modal footer → therefore analyze on [Annotated after action]. [Annotated after action] — target: OK pill; wrapping bbox: cyan-stroke OK+Cancel; traits: distinct controls = OK pill + Cancel pill; wrap count = 2; intended sub-target = OK pill.\n2 On [Annotated after action] — (a) cyan bbox; (b) cyan digit flush; (c) wraps OK pill; therefore selected overlay index 4.\n3 Exclusivity: wrap count 2 — multiple; route: coordinate path.\n4 Coordinate geometry: On [Annotated after action]: OK toward left of bbox 4; corner bottom-left; (xc,yc) from reference for 4; offset right+up; therefore (x,y) ≈ (xc+Δx, yc-Δy).\n\nTool route:\n1 Next recap & Location: Next recap: this turn: confirm dialog via OK pill; Location recap: coordinate path — overlay 4, multiple, aim ≈ (x,y).\n2 Tool call this turn: mouse:click_at at computed (x,y).",
  "headline": "Confirm dialog via OK coordinates",
  "tool_name": "mouse:click_at",
  "tool_args": { "goal": "Confirm dialog via OK pill", "action": "click OK pill center", "x": 520, "y": 880 }
}
```
