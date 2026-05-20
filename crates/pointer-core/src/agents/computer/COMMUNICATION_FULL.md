## Author reference — not loaded at runtime

**Runtime spec:** **`COMMUNICATION.md`** (slim) + **`COMMUNICATION_SHARED.md`** (image slots).

**This file** holds extended examples, anti-patterns, legacy notes, and authoring rationale.
**On-wire behavior always follows runtime `COMMUNICATION.md`.**

### Refactor notes (2026)

Runtime prompt is organized as:

1. **Proof discipline** — numbered lines, no skipped steps, conclusion only after evidence.
2. **Image discipline** — every visual claim cites **`On [Frame]:`**; frame registry table.
3. **Per-stage steps** — strict step tables (P1–P3, V0–V8, L-F1–L-4, T1–T3).
4. **Routing tables** — Location paths L0–L3, Verify lookup, Tool route execution.
5. **One golden example per stage** in runtime; **extended examples below**.

**Inject slots:** when a prior turn exists — **`[Screen before action]`** → **`[Zoom pointer before action]`** → **`[Screen after action]`** → **`[Annotated after action]`** → three **after** zooms.

---

## `<thoughts>` (legacy header — see runtime COMMUNICATION.md)

Runtime **COMMUNICATION.md** specifies **JSON** output: put the **full seven-stage block** (**`Pointer:`** … **`Tool route:`**; include **`Recheck coordinates:`** when **(x,y)** apply) inside **`thoughts`**; keep **`headline`** short. Integer **`x`/`y`** in **`tool_args`** — no floats.

**Verify** uses **12-row** lookup. Sections below may describe older **VERIFIED/NFO** wording — follow **COMMUNICATION.md** for on-wire behavior.

## Reasoning framework (mandatory every tool or final turn)

Run **seven** internal stages **in this order**, using **exactly** these **English prefix lines** so the chain is scannable:

- **`Pointer:`** — stage 1  
- **`Verify:`** — stage 2  
- **`Repetition:`** — stage 3  
- **`Next:`** — stage 4  
- **`Location:`** — stage 5 (overlay analysis; **`n/a`** when no overlay work)
- **`Recheck coordinates:`** — stage 6 (**only** when stage 5 has **(x,y)**; **R1** precision_miss 3px guard + **R2** target-at-point vs **Next** line 2)
- **`Tool route:`** — stage 7 (explicit tool call; line **2** = root **`tool_name`**)

### No speculation

- Judge only from **current injects** (`[CUR_SCREEN]` slots, **`[Recent desktop tool calls]`**, latest **tool result text** for this turn).
- **Do not** infer success from memory, generic product behavior, or “usually it copies.”
- **Do not** tell the user the clipboard contains a value unless **`clipboard:read`** (or on-screen text) **shows** it.
- If the prior step was **`clipboard:read`**, you may treat the returned text as **grounded evidence** in **`Verify:`** for that read step—not as proof of an earlier click unless the text matches a stated expectation.

### Overlay index discipline

**Within** internal stages **1–4** (**Pointer** through **Next**):
never use overlay **`index`**, digits, or “bbox N” / badge-only wording
(indices reset; **`index`** appears **only** inside **`Location:`**, stage **5**).

### Tool methods: overlay **index** vs **coordinates** (computer)

Same split as runtime **COMMUNICATION.md**: **index-based** calls use overlay digits on **`[Annotated after action]`**; **coordinate-based** calls use **`x`/`y`** with **Pointer position** + **`[Zoom pointer after action]`** as the coordinate anchor crop. **`clipboard:read`**, **`clipboard:write`**, **`hotkey`**, **`wait`**, **`response`**, and pointer-only **`mouse:…_current`** / **`move_offset`** / **`type_text_at_focused`** are neither.

---

### 1) Pointer

**Purpose (runtime):** Judge **pointer hotspot vs intended control center** on **`[Zoom pointer before action]`** when injected (**4×** magnified **±50 px** crop from **`[Screen before action]`**). **`[Screen before action]`** = pre-action layout + **current** pointer (name the aim on **line 1**). **Do not** compare before/after UI change in **`Pointer:`** (**`Verify:`** **`Before vs after`** only). When **no** before inject (first capture), use **`[Screen after action]`** / **`[Zoom pointer after action]`**. **Caret** is out of scope. **Coordinate** `*_at` anchoring remains **`[Zoom pointer after action]`** (300×300 annotated) — see runtime **COMMUNICATION.md** § Tool geometry.

**Conclusion** (runtime): **`accurate` | `abnormal` | `n/a`** per **Center-only rule** on the geometry image — **not** **`n/a`** because the target vanished on **after** while **before** zoom still shows aim + pointer.

**Legacy note:** Older paragraphs below used **`View vs before`** and **`[Screen after action]`** only; prefer runtime **COMMUNICATION.md** numbered **`Pointer:`** chain (**lines 1–2 + `3 Conclusion`**).

**Required form (fill in; one block per turn)**

```text
Pointer:
View vs before: <unchanged | changed — one short unindexed cue>.
Intended target for prior step: <label/shape/region; no overlay digits>.
Evidence from [Screen after action] (use [Zoom pointer after action] if needed): <where the cursor sits vs that target>.
Conclusion: <accurate | abnormal | n/a> — <one short reason>.
```

**Positive example**

```text
Pointer:
View unchanged vs before. Intended target: download arrow icon on an attachment row in the Downloads list.
From [Screen after action]: pointer sits on the filename text, left of the download icon. Conclusion: abnormal.
```

---

### 2) Verify

**Purpose:** Judge the **immediately previous automated step** using **Pointer** + **`[Screen before action]`** / **`[Screen after action]`** + any **grounded** tool output for that step.

**Visible evidence (step A)** — state mentally before outcomes:

- **Concrete visible outcome** — something **on screen** clearly changed or appeared (dialog, error, new panel, typed text, selection, scroll band, toast, etc.).
- **`no visible outcome`** — the task-relevant canvas looks **unchanged** for what that step was supposed to prove (blank click, no toast, no new row, etc.).

**Task type (step B)** — exactly one of:

- **`deferred`** — The step’s **success or failure** is meant to show **later** or on **another surface**: launch app, download, upload, export, print queue, background job, sync, save-and-continue without a completion banner, **or** any step whose **only** honest proof is **not** this screenshot (e.g. plain-text **clipboard** after **Copy** with **no** on-screen confirmation). Same spirit as Py **`task type: deferred`**.
- **`non-deferred`** — The step expects an **immediate on-canvas** result (open tab, focus field, dialog opens/closes, submit error, toggle state). Same spirit as Py **`task type: non-deferred`**.

**Outcome (step C)** — exactly **one** label; apply in order:

1. If **visible evidence** **contradicts** the intended action (wrong dialog, wrong panel, error that blocks the intent, or **`Pointer:`** **`abnormal`** when the step required hitting a **specific small control**) → **`FAILED`** (Py **`misidentified`** / wrong visible path rolls up here).
2. Else if **`no visible outcome`** **and** **`deferred`** **and** no strong visible “wrong operation” signal → **`NFO`**. **Next** must gather evidence (**`wait`**, Transfers/history/queue, destination folder, **`clipboard:read`**, etc.) — **do not** claim success to the user; **do not** repeat the same **trigger** until failure is visible or a check surface proves the outcome (**no NFO lock** in this runtime; still follow this discipline).
3. Else if **`no visible outcome`** **and** **`non-deferred`** → **`FAILED`** (Py: **`non-deferred`** + **`no visible outcome`** → action not verified; treat as wrong / stalled immediate UI).
4. Else if **concrete visible outcome** **fully** matches the step’s intent **and** (for precision clicks) **`Pointer:`** was **`accurate`** → **`VERIFIED`**.
5. Else if **concrete** visible progress **toward** the step’s intent is present, but the **full** outcome the **`goal`** implied is **not** yet satisfied on **`[Screen after action]`**, and nothing visible **contradicts** the path so far → **`PARTIAL`**. (Example: “export **all** 10 files” shows **3** completed rows; wizard moved one pane but more panes remain.) **Do not** use **`PARTIAL`** when **`Visible evidence`** is **`no visible outcome`** on a **deferred** proof path — that remains **`NFO`**. **Do not** use **`PARTIAL`** for wrong UI — **`FAILED`**.
6. Else → **`FAILED`** or **`NFO`** by conservative reading of the screen.

**`NFO`** (**need further verification**) — Use **only** when rule **2** applies: **`deferred`** + **`no visible outcome`** + no clear wrong-operation evidence. It means **unverified**, not “probably worked.” **Do not** carry a lock across turns in this product; still route **`Next:`** to a real check surface before **`response`** claims success.

**`VERIFIED`** — Rule **4** matched: visible proof on **`[Screen after action]`** (or grounded prior-tool text when that tool **is** the proof step, e.g. **`clipboard:read`** on the **same** turn you judge) **and** precision-click steps require **`Pointer:`** **`accurate`**.

**`PARTIAL`** — Rule **5**: **concrete** partial progress, intent **not** fully met on this frame, **no** contradiction. **`PARTIAL`** is **not** “unverified off-screen” (**`NFO`**) and **not** “all done” (**`VERIFIED`**). **`response`** must **not** claim the **whole** user task finished while **`Verify:`** is **`PARTIAL`** for remaining scope.

**`FAILED`** — Rules **1**, **3**, or conservative **6**; includes **non-deferred** steps that still show **`no visible outcome`** when the UI should have updated on canvas.

**Clipboard / copy flows**

- **Copy** with **no** toast or on-screen status: success is **off-screen** (clipboard) → treat **task type** as **`deferred`** for verification → with **`no visible outcome`**, use **`NFO`** and then **`clipboard:read`** (or user paste) before asserting the payload was copied. **Do not** output **`VERIFIED`** from the screenshot alone.
- If **`Pointer:`** shows the click missed the intended small control (**`abnormal`** for that target) → **`FAILED`** (rule **1**), not **`NFO`**.

**Required form (fill in; one block per turn)**

```text
Verify:
Before vs after (if [Screen before action] exists): <one short visible delta or “same layout”>.
Visible evidence: <concrete visible outcome | no visible outcome> — <one unindexed cue>.
Task type: <deferred | non-deferred> — <one short reason>.
Grounded tool output for prior step (if any): <role only; do not paste secrets>.
Pointer alignment (echo one line): <accurate | abnormal | n/a — same as Pointer block>.
Outcome: <VERIFIED | PARTIAL | NFO | FAILED> — <ties to rules 1–6; no overlay digits>.
```

**Positive examples**

```text
Verify:
Before: Save dialog open. After: dialog gone; pointer on canvas. Visible evidence: concrete — dialog dismissed. Task type: non-deferred. Pointer: accurate — hotspot in main canvas region, clear of sidebar chrome. VERIFIED
```

```text
Verify:
After: same Downloads list; no new “Saved” row. Visible evidence: no visible outcome for download started. Task type: non-deferred for the click target. Pointer: abnormal — on filename text, not download icon. FAILED
```

```text
Verify:
After: same canvas; subtle upload spinner at toolbar edge. Visible evidence: concrete — spinner started; file count unchanged. Task type: deferred — completion on cloud activity surface. Pointer: accurate — hotspot on Upload control center. NFO — open activity/history panel before asserting upload finished.
```

---

### 3) Repetition

**`[Recent desktop tool calls]`** (inject lists up to **5** rows; **time order** top → bottom = **oldest → newest**; the **last** line is the **latest** call):
same **goal+action** (or same intent) without UI progress?
Count **consecutive** matches.
**>3** → **STUCK**, commit to a **different** tactic next.
Sameness = **goal/action text**, not index.

**Required form (fill in; one block per turn)**

```text
Repetition:
Recent rows: <same goal|action repeated N times consecutively | rows differ>.
After screen vs stuck pattern: <flat / advanced — unindexed cue>.
Verdict: <OK | STUCK> — <if STUCK: one tactic change hint without overlay digits>.
```

**Positive example**

```text
Repetition:
Rows differ; screenshot advanced. OK
```

**Positive example**

```text
Repetition:
Four consecutive same goal|action; after screen flat. STUCK >3; next hotkey or coords, not same click.
```

---

### 4) Next

One step; target = **traits on `[Screen after action]`** only (label/shape/place).

**No** **`[Annotated after action]`** in this block;
**no** zoom overlay digits, **no** **`index`**, **no** “bbox N” / “badge K” / “click N” —
those belong only in **`Location:`**.

If **`Verify:`** was **`NFO`** for a silent copy: **`Next:`** should prefer **`clipboard:read`** (or open a status/history surface) **before** repeating the same copy click.

**Required form (fill in; one block per turn)**

```text
Next:
Single step intent: <what to do next in plain UI words on [Screen after action]>.
Tool class (no method numbers here): <e.g. read clipboard | click primary control | type in field — still no overlay digits>.
```

**Positive example**

```text
Next:
Read clipboard to ground copy result; no overlay digits in this block.
```

---

### 5) Location

After **`Next:`** internally, when the chosen tool needs a screen target—**do not** jump straight to “use **`index`** N”.

**BBox:** Each overlay **`index`** labels **one** axis-aligned **bbox**. **Pairing rule:** count digit ↔ **bbox** as matched **only** when **both** hold:
(a) **background color** behind the digit **matches** that **bbox**’s **border color**;
(b) the digit sits **tightly on** the **bbox** border—**flush** with the stroke, **not** floating between two **bbox** regions.
Same **bbox** for border, placement, digit-on-edge, and **`traits inside that bbox`**. In **`Location:`**, use **bbox** only so every line names the same shape.

**Target element features:** When describing the candidate bbox, include the target element’s **text** (exact or partial), **shape** (pill, chip, row, tab, field, glyph, etc.), **color or emphasis** if it disambiguates, **screen position** (band/region), and **neighbor features** (e.g. “left of Save”, “under error banner”). These features come from the **`Next:`** intent and are used to **match** or **mismatch** in step 3.

**Required form (fill in; numbered lines inside `Location:`; 5 lines)**

```text
Location:
1 <[Annotated after action] | [Zoom top after action] | [Zoom bottom after action] | [Zoom pointer after action]> index <N>: <target element text, shape, color, screen position, neighbors; bbox border color match; digit on bbox edge; vs other indices>.
2 Inside bbox: <traits — distinct controls, wrap count, intended sub-target>.
3 match|mismatch: <compare Next Intent features: text, shape, color, screen position, neighbors — inclusive decision>.
4 single|multiple: <distinct hit targets inside that bbox — count only after match>.
5 index <N> | coordinates | hover: <choice; if coords/hover, why>.
```

**Rules per line (details; same numbering as Required form):** 
**First**, from the reference frames ([Annotated after action] or zoom crops), pick a candidate index whose bbox color matches the digit background. 
**Line 1**: Name the frame and index, then describe the **target element** inside that bbox using its key features (text, shape, color, position, neighbors), as well as the bbox’s own traits (border color match, digit placement, bbox position, vs other indices).
**Line 2**: Inventory of all controls inside that bbox (feeds step 4 only; not used for match/mismatch).
**Line 3**: **Compare** the target element features from Line 1 with the Next Intent’s target features (text, shape, color, screen position, neighbors). If **all** features match → **match** (even if the bbox wraps extra UI). If **any** feature differs → **mismatch**: reject this index, go back to Line 1 with a **different candidate** whose features are closer to the Next Intent.
**Line 4**: Run only after **match**. Route from **`traits`** **`wrap count`** on line 1 (not re-counted on line 3).
**Line 5**: Map step 3–4: **match + single** → **`index`**; **match + multiple** → **coordinates** (or hover/defer); if no match after exhaustive tries → **coordinates** on [Screen after action].

**Exclusivity after match:**
- single-element: only this index (route: use index)
- multi-elements: multiple distinct targets inside the bbox (route: use coordinates)

**Positive example** — Single **bbox**, single target: `3 match` + `4 single` → `5 index`.

```text
Location:
1 [Zoom pointer after action] index 4: target = ⋯ chip (text: ⋯, shape: pill, band: row title area, neighbors: right of title). bbox border color magenta; tight bbox hugging ⋯ chip; not touching index-3 bbox.
2 Inside bbox: ⋯ only.
3 match: ⋯ pill matches Next intent features (pill shape, row title band, right of title).
4 single.
5 index 4
```

**Positive example** — OK + Cancel in footer strip: `3 match` + `4 multiple` → `5 coordinates`.

```text
Location:
1 [Annotated after action] index 4: target = OK pill (text: OK, shape: blue pill, band: dialog footer, neighbors: left of Cancel pill). bbox border color cyan; footer bar spans OK + Cancel; digit on footer edge.
2 Inside bbox: OK pill, Cancel pill — multiple.
3 match: OK features match (blue pill, footer band, left of Cancel).
4 multiple.
5 coordinates: OK pill center; not index 4
```

**Mismatch example** — Retry.

```text
Location:
1 [Annotated after action] index 7: target = gear icon (shape: gear, color: gray, band: footer, neighbors: left of index 10 strip). bbox border color green; bbox spans left rail + first table row — footer gear not inside.
2 Inside bbox: rail + row clutter.
3 mismatch: gear icon not inside this bbox (target in footer, this bbox covers rail/row) — pick another index.
```

**After exhaustive mismatch** (no match found):

```text
Location:
1–4: tried five candidates; each inventory vs Next intent → mismatch; no exclusive single-element index for small gear alone.
5 hover: hover strongest toolbar candidate — defer until next turn.
```

In real replies, **each** failed candidate should still use a full **line 1** (frame + index + target features + bbox traits); the line above compresses five tries for brevity.

---

### Full chain

**Positive example** — Screen-targeted tool turn (five prefixes + Location lines 1–6 inside the **`thoughts`** string):

```json
{
  "thoughts": "Pointer:\nView unchanged. Intended: gray Submit in dialog. Zoom: hotspot on Submit pill center. Conclusion: accurate.\n\nVerify:\nBefore: invalid-field banner. After: same banner, Submit still enabled. Visible evidence: no visible outcome for submit completing. Task type: non-deferred. Pointer: accurate — hotspot on Submit center. FAILED — submit did not complete.\n\nRepetition:\nLast two rows differ in action text; UI not flat over four same rows. OK\n\nNext:\nClick gray Submit under password fields; full column width; not Cancel link; no overlay digits in this block.\n\nLocation:\n1 [Screen after action]: gray Submit under password fields.\n2 [Annotated after action] index 6: green tall card **bbox** around the form stack; **background color** behind **6** matches green **bbox** border color.\n3 Inside bbox: Email field, password fields, gray Submit — multiple distinct hit targets.\n4 match: Submit inside index 6 bbox.\n5 multiple.\n6 coordinates: Submit pill center; not index 6",
  "headline": "Retry submit via coordinates",
  "tool_name": "mouse:click_at",
  "tool_args": {}
}
```

**Positive example** — `wait` or **`clipboard`** turn (no screen coordinates): **omit** the entire **`Location:`** block inside **`thoughts`**:

```json
{
  "thoughts": "Pointer:\nn/a — this turn is clipboard:read only.\n\nVerify:\nAfter tool reply is in the thread: visible evidence for this read step is grounded tool text (clipboard payload). Task type: non-deferred for clipboard:read itself. Pointer: n/a. VERIFIED for clipboard read evidence only.\n\nRepetition:\nOK\n\nNext:\nclipboard:read — read system clipboard; no overlay digits in this block.",
  "headline": "Read clipboard",
  "tool_name": "clipboard:read",
  "tool_args": {}
}
```

---

## Extended Location examples (author reference)

Moved from runtime **COMMUNICATION.md** to save tokens. Runtime keeps compose+113 and OK pill only.

### Search icon (nearest = center)

```text
Location:
1 … band=top; text=Search; fill=orange icon; size=≈24×24 px.
2 … digit 7 flush on orange bbox; Step 2 band=top; text=Search; fill=orange circle; size=≈24×24 px.
   Step 3: inside bbox 7; vs bbox R center.
3 … Bearing check consistent; anchor = center point; direction = on anchor;
   Overlay reference bboxes row 7 center: (xa, ya) = (512, 48); offset none → (512, 48).
```

### Back chevron above-left of bbox 67

```text
2 Step 2: band=top; text=[unclear]; fill=gray chevron; size=≈24×24 px.
   Step 3: outside bbox 67; above-left vs bbox 67 top-left corner.
3 anchor = top-left corner; direction = up-left; offset Δx=−18, Δy=−32 → (6, 24).
```

### Menu icon top-left inside bbox 22

```text
2 Step 3: inside bbox 22; vs bbox 22 top-left corner.
3 anchor = top-left corner; direction = on anchor → (12, 8).
```

### Send button bottom-left inside bbox 101

```text
2 Step 2: band=bottom; text=Send; fill=blue pill; size=≈48×28 px.
   Step 3: inside bbox 101; vs bbox 101 bottom-left corner.
3 anchor = bottom-left corner; direction = on anchor → (890, 855).
```

---

## Extended anti-patterns (author reference)

Violations of runtime **proof discipline** and **image discipline**. Normative rules: **`COMMUNICATION.md`**.

### Pointer — skip line 2 or verdict before evidence

**Forbidden — line 3 before line 2:**

```text
Pointer:
1 Intended aim on [Screen before action]: Bluetooth toggle knob; aim = knob center.
3 Conclusion (Center-only rule): abnormal — (forbidden: Evidence missing).
2 Evidence (hotspot vs aim): On [Zoom pointer before action]: …
```

### Verify — no frame anchor or Step result before Lookup

**Forbidden — vague Before vs after:**

```text
Before vs after: Text appears in search bar.
(must name On [Screen before action]: … On [Screen after action]: …)
```

### Next — tool name in line 2

**Forbidden:**

```text
2 Target: locate thread — might be personal chat.
Tool kind: mouse click.
```

### Location — overlay digit on line 1

**Forbidden:**

```text
1 … neighbors: left of Messages (index 34) …
```

### Tool route — contradicts Location

**Forbidden:**

```text
Tool route: hover_index on 28.
"tool_name": "mouse:click_at"
```

