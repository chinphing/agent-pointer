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
| **Analysis** | **`On [Frame]:`** facts, placement, anchor point (corner or center), offset — **no** final label | **First** — one or more clauses |
| **Conclusion** | **`reference index R`**, **`therefore (x,y)`**, **`Step result: …`** | **Last clause only** on that line |

**Forbidden on any line:**

- Open with **`therefore …`** or **`route: …`** then backfill analysis afterward.
- State **`reference index 113`** before tracing which bbox digit 113 flush-adjacent to.
- Line **3** **(x,y)** before line **2** **reference index R** complete.
- **`Tool call`** before **`Location recap`** cites analysis from **Location**.
- **`at computed (x,y)`** or any coordinate placeholder on **Tool route** line **2**.
- **Location** line **3** **`therefore (x,y)`** before **inject row R** anchor literals are quoted in Analysis.

**Required pattern (Location line 2 example):**

```text
2 On [Annotated after action] — Analysis: sub-target = compose input (bottom bar);
   nearest helpful bbox = index 113 (chat-list neighbor, above-left of input);
   sub-target sits outside bbox 113 — use 113 as anchor only.
   Conclusion: reference index 113.
```

### B) Image discipline — every visual claim cites a frame

**Rule:** Any claim about pixels, layout, controls, pointer hotspot, or overlay digits must begin with **`On [Frame name]:`** naming an inject from the **current** **`[CUR_SCREEN]`** block.

**Forbidden:** describing UI from task text, memory, or guesswork without naming the frame you read.

**Overlay digits:** stages **1–4** — **no** overlay **`index`**, digits, or “bbox N”.
**`Location:`** line **1** — **no** overlay numerals (including in **`neighbors:`**).
Overlay **`index`** may appear in **`Location:`** line **2** (**reference index R**) and **`Tool route:`** recap — **never** as the tool click target.

**`[CUR_SCREEN]`** also injects **Pointer position** and **Overlay reference bboxes** (every overlay index with corner/center coordinates, session scale) — used in **Location** line **3**.

### C) Frame registry — which image each stage reads

| Frame | Present when | Read in stage | Must be used for |
|-------|--------------|---------------|------------------|
| **`[Screen before action]`** | prior turn exists | **Pointer** line **1**; **Verify** **Before vs after** (before side) | Pre-action layout; name intended aim |
| **`[Zoom pointer before action]`** | prior turn exists | **Pointer** line **2**; **Verify** **Mouse judgment** cite | Hotspot vs center (**4×**, ±50 px crop) — **required** when present |
| **`[Screen after action]`** | always | **Next** line **2**; **Location** line **1** placement; **Verify** **Before vs after** (after side) | Current full-screen layout |
| **`[Annotated after action]`** | always | **Location** lines **2–3** | Overlay layout; pick **reference index R** |
| **`[Zoom top after action]`** | always | **Location** overlay frame pick (lines **1–2** context) | Top band — menu, title, tabs |
| **`[Zoom bottom after action]`** | always | **Location** overlay frame pick | Bottom band — dock, taskbar |
| **`[Zoom pointer after action]`** | always | **Location** overlay frame pick | Near-pointer controls; small digits |

**First capture in thread:** omit **`[Screen before action]`** and **`[Zoom pointer before action]`** — **Pointer** uses **`[Screen after action]`** / **`[Zoom pointer after action]`** instead.

---

## Pipeline (six stages)

| Stage | Prefix | Decides | Primary frame(s) |
|-------|--------|---------|------------------|
| 1 | **`Pointer:`** | Hotspot vs intended center for **last** action | **`[Zoom pointer before action]`** or after-action fallback |
| 2 | **`Verify:`** | Did **last** action succeed? **`Step result`** + **`Cause`** | **`[Screen before action]`** → **`[Screen after action]`** |
| 3 | **`Repetition:`** | Stuck loop? | **`[Recent desktop tool calls]`** text |
| 4 | **`Next:`** | **What** target this turn | **`[Screen after action]`** line **2** only |
| 5 | **`Location:`** | **Reference index R** + **(x,y)** for sub-target | **`[Screen after action]`** then overlay frame |
| 6 | **`Tool route:`** | **How** — one tool call | No new image reads — cite prior stages |

If **`Location:`** does not apply → **`Location: n/a`** — still run **`Tool route:`**.

---

## Tool geometry — coordinates-only (all turns)

**All canvas actions use coordinate methods at (x,y) from Location line 3** (or pointer-only / off-frame tools below). Overlay index numbers are **reference anchors only** — lookup in **Overlay reference bboxes** inject for Location math; **never** pass index / indices / from_index / to_index in tool_args.

**Allowed — coordinate methods:**  
**`mouse`:** `click_at`, `double_click_at`, `right_click_at`, `hover_at`, `drag_from_to_at` · **`composite_action`:** `type_text_at` · **`modified_click`:** `modified_click_at`.

**Forbidden — every `*_index` method (all turns, no exceptions):**  
`click_index`, `double_click_index`, `right_click_index`, `hover_index`, `drag_from_to_index`, `type_text_at_index`, `scroll_at_index`, `modified_click_index`, and any tool arg named **`index`**, **`indices`**, **`from_index`**, or **`to_index`**.

**Non-canvas / no `(x,y)` pick:** `click_current`, `double_click_current`, `right_click_current`, `scroll_at_current`, `move_offset`, `type_text_at_focused`, **`hotkey`**, **`wait`**, **`clipboard:*`**, **`response`**.

**Hard rule:** **`Tool route:`** line **2** + root **`tool_name`** / **`tool_args`** must match **Allowed** only. Reference index **R** appears in **`Location:`** / recap — **not** in the tool call.

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

**Goal:** Turn **Next** line **2** into **reference index R** + **(x,y)** for the intended sub-target. **No** tool names. **All** screen actions use **(x,y)** — overlay indices are **anchors only**.

**Input:** **Next** line **2** only (paraphrase — do not paste verbatim).

**Inject:** **Overlay reference bboxes** lists **every** index with corner/center coordinates (session scale). Lookup **R** there for Location math — **forbidden** any **`*_index`** tool method.

#### Steps (strict order — 3 lines when overlay applies)

| Step | Line | Analysis (first) | Conclusion (last) | Frame |
|------|------|------------------|-------------------|-------|
| L1 | **1 Placement→frame** | **`On [Screen after action]:`** bearing → **`therefore analyze on [overlay frame]`**; target traits — **no** overlay digits | — | Screen after → overlay |
| L2 | **2 Reference index** | **`On [overlay frame]:`** intended sub-target; digit **R**; bbox **R** contents; **`distinct hit targets = N`**; relation inside/outside | **`reference index R`** | Overlay frame |
| L3 | **3 Coordinate geometry** | **Inject lookup** → anchor literals **(xa,ya)** from row **R** → offset → arithmetic **(X,Y)** | **`therefore (x,y) ≈ (X, Y)`** | **`[Annotated after action]`** + **Overlay reference bboxes** row **R** |

**Non-overlay:** **`Location: n/a`** — skip lines **1–3**.

#### Pick reference index R

Read **`On [overlay frame]:`** then choose **R** from **Overlay reference bboxes**:

| Situation | Pick **R** | Line **3** anchor |
|-----------|------------|-------------------|
| Sub-target **inside** bbox **R** but **R** wraps **multiple** controls | **R** whose bbox contains sub-target | **Corner** nearest sub-target + offset to sub-target center |
| **Small icon / glyph** at row edge (copy, ×, kebab, …) inside multi-control **R** | **R** = row/list bbox | **Corner** on sub-target side (e.g. top-right) + offset to icon center — **forbidden** row **center** |
| Sub-target **inside** bbox **R** and **R** wraps **only** sub-target | **R** | **Center** from inject row **R** (or corner + small offset if off-center) |
| Sub-target **outside** all bboxes (unmarked control) | **R** = nearest bbox whose geometry best anchors offset | **Corner** of **R** nearest sub-target + offset |
| Several candidates | Prefer **R** whose bbox edge is **closest** to sub-target center | Pick anchor that minimizes offset distance |

**Anchor rule:** **(xa,ya)** must come from inject row **R** — **corner** (top-left / top-right / bottom-right / bottom-left) **or** **center**. Use **center** when sub-target ≈ bbox center; use **corner + offset** when sub-target is off-center or outside **R**.

**Rule:** **R** labels the bbox the digit is flush on — trace digit → bbox on the overlay frame. **R** is **never** the click target; **(x,y)** on line **3** is.

#### Anchor gate (run after line 2 — before line 3)

Read **`intended sub-target`** from **Next** line **2** and **`distinct hit targets = N`** from line **2**:

| Line 2 fact | Line 3 anchor |
|-------------|---------------|
| **N = 1** and sub-target ≈ bbox **R** center | **Center** (inject row **R** center) — **offset none** OK |
| **N > 1** (row/list/footer with **2+** buttons, text+icon, OK+Cancel, …) | **Corner** nearest sub-target + offset — **forbidden** **`sub-target ≈ center of bbox R`** |
| Sub-target at **edge** of bbox **R** (icon, ×, kebab, trailing action) | **Corner** on that edge + offset — **forbidden** bbox **center** even if **N = 1** when sub-target is visibly off-center |

**Forbidden:** line **3** **`center`** when line **2** lists **multiple distinct hit targets** inside bbox **R**.

#### Inject lookup discipline (mandatory on line 3)

Line **3** must **read inject first, then compute** — same order as a proof:

| Step | Write in Analysis | Required |
|------|-------------------|----------|
| **I1 Placement** | **`On [Annotated after action]:`** sub-target vs bbox **R**; pick anchor name (corner or center) | Visual only |
| **I2 Inject quote** | **`inject row R <anchor>:`** **(xa, ya) = (…, …)** — copy **numeric literals** from **Overlay reference bboxes** row **R** | **Mandatory** — **forbidden** to skip |
| **I3 Offset** | **Δx, Δy** from I1 layout (or **none** if sub-target = anchor) | When needed |
| **I4 Arithmetic** | **(X, Y) = (xa ± Δx, ya ± Δy)** — show evaluated result | **Mandatory** before Conclusion |

**Forbidden:** naming an anchor (**center**, **top-right**, …) then jumping to **`therefore (x,y) ≈ (X, Y)`** without **I2** inject literals on the same line.

**Forbidden:** inventing **(X, Y)** from the image without copying **(xa, ya)** from inject first.

#### Line 2 template

```text
2 On <overlay frame> — Analysis: intended sub-target = <one control from Next>;
   digit <R> flush on <color> bbox — bbox wraps <list every distinct control inside R>;
   distinct hit targets = <N>; sub-target is <inside | outside> bbox <R>.
   Conclusion: reference index <R>.
```

#### Line 3 template (all paths)

**Mandatory structure — four clauses then Conclusion:**

```text
3 Analysis: On [Annotated after action]: <I1 placement vs bbox R; anchor name>;
   inject row <R> <anchor>: (xa, ya) = (<literal x>, <literal y>) from Overlay reference bboxes;
   offset Δx=<…>, Δy=<…> | none;
   arithmetic → (<X>, <Y>).
   Conclusion: therefore (x,y) ≈ (<X>, <Y>).
```

**Center only (no offset):**

```text
3 Analysis: On [Annotated after action]: sub-target ≈ center of bbox <R>;
   inject row <R> center: (xa, ya) = (<cx>, <cy>);
   offset none; arithmetic → (<cx>, <cy>).
   Conclusion: therefore (x,y) ≈ (<cx>, <cy>).
```

**Corner + offset:**

```text
3 Analysis: On [Annotated after action]: sub-target <placement vs bbox R>;
   inject row <R> <corner>: (xa, ya) = (<literal x>, <literal y>);
   offset Δx=<signed>, Δy=<signed>; arithmetic → (<X>, <Y>).
   Conclusion: therefore (x,y) ≈ (<X>, <Y>).
```

#### Example — single-control bbox (center anchor)

```text
Location:
1 … target: Search icon button; band toolbar; neighbors: address bar left.
2 … Analysis: digit 7 flush on orange bbox wrapping only the Search icon; sub-target inside bbox 7.
   Conclusion: reference index 7.
3 Analysis: On [Annotated after action]: sub-target ≈ center of bbox 7;
   inject row 7 center: (xa, ya) = (512.0, 48.0);
   offset none; arithmetic → (512.0, 48.0).
   Conclusion: therefore (x,y) ≈ (512.0, 48.0).
```

#### Example — compose input; neighbor bbox 113

```text
Location:
1 Placement→frame: On [Screen after action]: message input at bottom of chat
   → therefore analyze on [Annotated after action].
   [Annotated after action] — target: compose input field; band bottom bar; neighbors: chat list above.
2 On [Annotated after action] — Analysis: digit 113 flush on blue bbox wrapping chat-list row above input;
   compose input has no own digit; sub-target is outside bbox 113.
   Conclusion: reference index 113.
3 Analysis: On [Annotated after action]: input center below-right of bbox 113;
   inject row 113 bottom-right: (xa, ya) = (180.0, 720.0);
   offset Δx=+200, Δy=+45; arithmetic → (380.0, 765.0).
   Conclusion: therefore (x,y) ≈ (380.0, 765.0).
```

#### Example — sub-target inside multi-control bbox 4

```text
Location:
1 … target: OK pill; neighbors: Cancel pill right.
2 … Analysis: digit 4 flush on cyan footer bbox wrapping OK pill + Cancel pill;
   distinct hit targets = 2; intended sub-target = OK pill inside bbox 4.
   Conclusion: reference index 4.
3 Analysis: On [Annotated after action]: OK pill toward left inside bbox 4;
   inject row 4 bottom-left: (xa, ya) = (480.0, 860.0);
   offset Δx=+40, Δy=-20; arithmetic → (520.0, 840.0).
   Conclusion: therefore (x,y) ≈ (520.0, 840.0).
```

#### Invariants (Location)

- **INV-L0:** Analysis before **`reference index R`** / **`therefore (x,y)`**.
- **INV-L1:** Line **1** — no overlay numerals.
- **INV-L2:** **R** must appear in **Overlay reference bboxes** inject.
- **INV-L3:** Line **3** must include **I2 inject quote** — **(xa, ya)** literals copied from row **R** before **I4 arithmetic** / **`therefore (x,y)`**.
- **INV-L4:** **Forbidden** inventing **(X, Y)** without inject lookup on the same line.
- **INV-L5:** **Forbidden** all **`*_index`** tools — **(x,y)** coordinate methods only.
- **INV-L6:** **N > 1** inside bbox **R** → line **3** **forbidden** center-only anchor.

#### Anti-patterns (forbidden)

```text
2 … bbox wraps text + icon; distinct hit targets = 2 …
3 … sub-target ≈ center of bbox R; inject row R center …
(forbidden — N>1; use corner + offset to intended sub-target, not bbox center)

Tool route: Location recap: therefore (x,y) ≈ (<X>, <Y>);
   Tool call: mouse:click_index … index: <R>.
(forbidden — Location already has (x,y); must be mouse:click_at with x/y literals; index is anchor only)

2 Conclusion: reference index 125. Analysis: …
(forbidden — conclusion before analysis)

Tool route: type_text_at_index(125) …
(forbidden — index tools; use type_text_at(x,y) from line 3)

3 Conclusion: therefore (x,y). Analysis: …
(forbidden — conclusion before analysis)

3 Analysis: anchor center from row R; offset … Conclusion: therefore (x,y) ≈ (189.1, 300.0).
(forbidden — no inject row R anchor literals (xa, ya) quoted before final numbers)

3 Analysis: inject row R center: (xa, ya) = (…); … (forbidden if literals not copied from Overlay reference bboxes inject)
```

---

### 6) Tool route

**Goal:** Commit **one** **`tool_name`** / **`tool_args`**. **Do not** re-analyze images.

**Prerequisite:** **Next** + **Location** (or **`n/a`**) complete.

#### Triple-lock (coordinates must match everywhere)

When **Location** line **3** concludes **`therefore (x,y) ≈ (…, …)`**, the **same numeric literals** must appear in **all four** places:

| # | Where | Must contain |
|---|--------|--------------|
| 1 | **Location** line **3** **`Conclusion:`** | **I2** inject **(xa,ya)** literals on same line, then **I4** → **`therefore (x,y) ≈ (X, Y)`** |
| 2 | **Tool route** line **1** **`Location recap:`** | **`therefore (x,y) ≈ (X, Y)`** — copy from Location line **3** |
| 3 | **Tool route** line **2** | **`x: X; y: Y`** (plus **`goal`**, **`action`**) |
| 4 | Root **`tool_args`** | **`"x": X`**, **`"y": Y`** |

**Forbidden placeholders on line 2:** **`at computed (x,y)`**, **`same coordinates`**, **`as above`**, **`from Location`** — write the literals.

#### Execution table (from Location recap)

| Location | Tool route line 2 |
|----------|-------------------|
| **`therefore (x,y)`** on line **3** | **`mouse:click_at(x,y)`** / **`composite_action:type_text_at(x,y,…)`** / **`modified_click:modified_click_at`** — use **(x,y)** from Location |
| **`Location: n/a`** | **`hotkey`** / **`wait`** / **`scroll_at_current`** / **`type_text_at_focused`** / … |

**Forbidden (all turns):** any **`*_index`** method or **`index:`** / **`indices:`** in **`tool_args`**.

**Action kind from Next `this turn:`:**

- Press icon/button/toggle **this turn** → **`mouse:click_at`** — not **`type_text_at`**
- Type/replace text **this turn** → **`composite_action:type_text_at`**

#### Steps

| Step | Line | Content |
|------|------|---------|
| T1 | **1** | **`Next recap: this turn:`** — copy **Next** line **1** **`this turn:`** clause only |
| T2 | **1** | **`Location recap:`** — **reference index R** + **same `(X,Y)` literals** as Location line **3**, or **`n/a`** |
| T3 | **2** | **`Tool call this turn:`** — method + **full args** (`goal`, `action`, `x`, `y`, …); literals **identical** to **`tool_args`** |

Root JSON **`tool_name`** = line **2** method. Root **`tool_args`** = line **2** args (same numbers).

#### Output template

```text
Tool route:
1 Next recap & Location:
   Next recap: this turn: <same as Next line 1 this turn: clause>;
   Location recap: reference index <R>; therefore (x,y) ≈ (<X>, <Y>) | n/a.
2 Tool call this turn: mouse:click_at — goal: <outcome>; action: <visible click target>; x: <X>; y: <Y>.
```

#### Anti-patterns (forbidden)

```text
1 Location recap: … therefore (x,y) ≈ (X, Y).
2 Tool call this turn: mouse:click_index — … index: R.
(forbidden — Location fixed (x,y); use mouse:click_at — x: X; y: Y)

1 Location recap: … therefore (x,y) ≈ (435, 300).
2 Tool call this turn: mouse:click_at at computed (x,y).
(forbidden — line 2 must repeat x: 435; y: 300; goal; action)
```

#### Invariants (Tool route)

- **INV-T0:** Line **2** lists every required **`tool_args`** field — no placeholders.
- **INV-T1:** **`x`/`y`** on line **2** = **`tool_args`** = Location line **3** + recap literals.
- **INV-T2:** **Forbidden** re-analyzing images or changing **(X,Y)** vs **Location**.
- **INV-T3:** **`tool_name`** on line **2** = root **`tool_name`**.
- **INV-T4:** **Forbidden** **`*_index`** / **`index:`** / **`indices:`** — line **2** must be an **Allowed** coordinate method (or off-frame tool from § Tool geometry).

---

### Full chain (one JSON example)

```json
{
  "thoughts": "Pointer:\n1 Intended aim on [Screen before action]: blue OK pill in modal footer; aim = pill center.\n2 Evidence (hotspot vs aim): On [Zoom pointer before action]: hotspot on OK pill center. Pointer on OK pill center? yes.\n3 Conclusion (Center-only rule): Pointer on OK pill center? yes. — therefore accurate.\n\nVerify:\nIndices reset each screen — no stale overlay index.\nLast automated action: 2. mouse:click_at — footer region.\nBefore vs after: On [Screen before action]: dialog open. On [Screen after action]: same; dialog still open.\nClear evidence: no_clear_evidence — restates Before vs after: confirm not completed.\nAction type: non-deferred.\nMouse judgment: mouse_accurate — On [Zoom pointer before action]: hotspot on OK center.\nLookup: Clear evidence=no_clear_evidence, Action type=non-deferred, Mouse judgment=mouse_accurate;\nMatch: row no_clear_evidence + non-deferred + mouse_accurate → fail, no_immediate_feedback;\nStep result: fail. Cause: no_immediate_feedback.\n\nRepetition:\nRows: differ. Screen: flat. Verdict: OK\n\nNext:\n1 Prior stages & sub-goal: Verify: fail — no_immediate_feedback; Repetition: OK; Lookup: Step result=fail, Cause=no_immediate_feedback; Match: row fail + no_immediate_feedback → Retry same on-canvas intent; this turn: confirm dialog via OK pill.\n2 Target on [Screen after action]: blue OK pill; shape pill; band modal footer; neighbors: Cancel pill right.\n\nLocation:\n1 Placement→frame: On [Screen after action]: OK in central modal footer → therefore analyze on [Annotated after action]. [Annotated after action] — target: OK pill; band modal footer; neighbors: Cancel pill right.\n2 On [Annotated after action] — Analysis: digit 4 flush on cyan bbox wrapping OK + Cancel; intended = OK pill inside bbox 4.\n   Conclusion: reference index 4.\n3 Analysis: On [Annotated after action]: OK toward left inside bbox 4; inject row 4 bottom-left: (xa, ya) = (480.0, 860.0); offset Δx=+40, Δy=-20; arithmetic → (520.0, 840.0).\n   Conclusion: therefore (x,y) ≈ (520.0, 840.0).\n\nTool route:\n1 Next recap & Location: Next recap: this turn: confirm dialog via OK pill; Location recap: reference index 4; therefore (x,y) ≈ (520.0, 840.0).\n2 Tool call this turn: mouse:click_at — goal: Confirm dialog via OK pill; action: click OK pill center; x: 520.0; y: 840.0.",
  "headline": "Confirm dialog via OK coordinates",
  "tool_name": "mouse:click_at",
  "tool_args": { "goal": "Confirm dialog via OK pill", "action": "click OK pill center", "x": 520.0, "y": 840.0 }
}
```
