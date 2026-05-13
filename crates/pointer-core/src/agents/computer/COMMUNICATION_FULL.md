## `<thoughts>`

Runtime **COMMUNICATION.md** (slim) specifies **JSON** output: put the **full five-stage block** (**`Pointer:`** … optional **`Location:`**`) inside the **`thoughts`** string field; keep **`headline`** a **short** label. This file documents the same stages in more detail for authors.

## Reasoning framework (mandatory every tool or final turn)

Run **five** internal stages **in this order**, using **exactly** these **English prefix lines** so the chain is scannable:

- **`Pointer:`** — stage 1  
- **`Verify:`** — stage 2  
- **`Repetition:`** — stage 3  
- **`Next:`** — stage 4  
- **`Location:`** — stage 5 (only when this turn chooses a **screen-targeted** tool: `mouse` / `hotkey` / `composite_action` / `modified_click`; **omit** the whole **`Location:`** block when `next:` is `wait`, `clipboard`, or **`response`**)

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

Same split as runtime **COMMUNICATION.md**: **index-based** calls use overlay digits on **`[Annotated after action]`**; **coordinate-based** calls use **`x`/`y`** (and optional **pointer neighbor reference bbox** entries in **`[CUR_SCREEN]`** text — anchors for coordinates only). **`clipboard:read`**, **`clipboard:write`**, **`hotkey`**, **`wait`**, **`response`**, and pointer-only **`mouse:…_current`** / **`move_offset`** / **`type_text_at_focused`** are neither.

---

### 1) Pointer

**Purpose:** From **`[Screen after action]`** (synthetic pointer/caret on the capture), judge whether the **pointer hotspot** matches the **prior step’s intended aim point**—geometry only, **not** overall task success. Coordinate **`x`/`y`** and overlay **`index`** clicks land on the **center** of the chosen control; **inside** the same field or strip without **center** alignment is **not** enough.

**Source:** Use the **cursor/caret overlay** on **`[Screen after action]`** only. Prefer **`[Zoom pointer after action]`** to see whether the hotspot sits on the **center** (or expected caret) vs an edge or wrong sub-control. Do **not** use **`[Annotated after action]`** digit positions as a substitute for “where the pointer is.”

**Branch on view delta** (when **`[Screen before action]`** exists): note whether task-relevant UI **changed** vs before; then still judge pointer vs **that step’s** target widget (label/shape/region), without overlay numbers.

**Conclusion** (one of):

- **`accurate`** — The **hotspot** (click point) aligns with the **intended aim**: for compact controls (buttons, icons, toggles, inputs, address/search bars), that is the **control center** within a **small** visible tolerance. For **type-at-focus** steps, the **caret/I-beam** sits at the expected **insertion** point.
- **`abnormal`** — The hotspot is **near** but **off-center** on the same widget, on a **wrong sub-part** (e.g. row label vs trash icon), **inside** a wide field but **not** at center, or visibly offset above/below/left/right of the aim. “Near” counts as **abnormal**, not **accurate**.
- **`n/a`** — The prior target is **not** visible after a view change, or the cursor overlay is **missing** on **`[Screen after action]`** so position cannot be judged.

**Do not** use prior tool JSON, overlay indices, or “we clicked N” to set **`accurate`**; only pixel evidence on **`[Screen after action]`** (and pointer zoom).

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
View unchanged vs before. Intended target: small copy-to-clipboard icon right of masked API key text.
From [Screen after action]: pointer sits on the key text, clearly right of the copy icon. Conclusion: abnormal.
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

- **Copy** with **no** toast or on-screen status: success is **off-screen** (clipboard) → treat **task type** as **`deferred`** for verification → with **`no visible outcome`**, use **`NFO`** and then **`clipboard:read`** (or user paste) before asserting the key was copied. **Do not** output **`VERIFIED`** from the screenshot alone.
- If **`Pointer:`** shows the click missed the copy icon (**`abnormal`** for that target) → **`FAILED`** (rule **1**), not **`NFO`**.

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
After: same API keys page; no toast. Visible evidence: no visible outcome for copy success. Task type: non-deferred for the click target. Pointer: abnormal — on key text, not copy icon. FAILED
```

```text
Verify:
After: same page; no toast after copy click. Visible evidence: no visible outcome for clipboard payload. Task type: deferred — proof surface is clipboard, not canvas. Pointer: accurate — hotspot on copy icon center. NFO — next clipboard:read or user paste before asserting success.
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
Same **bbox** for border, placement, digit-on-edge, and “inside” on inventory lines. In **`Location:`**, use **bbox** only so every line names the same shape.

**Required form (fill in; numbered lines inside `Location:`)**

```text
Location:
1 [Screen after action]: <target widget in full-screen words — same intent as Next; no overlay digits>.
2 <[Annotated after action] | [Zoom top after action] | [Zoom bottom after action] | [Zoom pointer after action]> index <N>: <background color behind index matches bbox border color; digit tightly on bbox border; bbox placement; digit on bbox edge vs neighbors>.
3 Inside bbox: <inventory of wrapped controls — feeds step 5 only>.
4 match|mismatch: <one line — intended target inside|outside this index bbox; if mismatch, later lines retry another index>.
5 single|multiple: <one line — hit-target count inside that bbox>.
6 index <N> | coordinates: <x,y verbal or “center of …”; if coordinates, say why index unsafe>.
```

**Rules per line (details; same numbering as Required form):** **First** locate the control from **`Next:`** on **`[Screen after action]`** (step 1). **Step 4** **only** checks **inclusion**: is the step-1 target **inside** this candidate **`index`** bbox? **Step 5** **only** counts **one** vs **multiple** targets inside **that** bbox (after step 4 matched). **Step 6** **only** maps steps 4–5 to **`index`** or **coordinates**. Step 4 **must not** judge inner multiplicity; step 5 **must not** decide inclusion; **coordinates** are never chosen inside step 4.

1. **Confirm target (first, full screen):** On **`[Screen after action]`** only, locate the control from **`Next:`**
   (label, shape, band, neighbors). State clearly: **this** widget is the aim—same role and place as stage 4.
   If it is missing or ambiguous, fix **`Next:`** mentally or switch tactic before any overlay reasoning.
2. **Candidate overlay + frame:** Name the frame—**`[Annotated after action]`**, **`[Zoom top after action]`**, **`[Zoom bottom after action]`**, or **`[Zoom pointer after action]`**—whichever shows the **`index`** and **bbox** most clearly (**`[Zoom pointer after action]`** when the aim is near the pointer; **top**/**bottom** zooms when the target sits in the menu bar / title strip or dock / taskbar). **Then** cite a **candidate** **`index`** whose **bbox** **may** contain the step-1 target, and state the **bbox**’s own traits:
   **background color** behind the printed index **matches** this **bbox**’s **border color** (digit glyph ink may differ);
   digit **tightly on** that **bbox** border—**flush**, **not** between two **bbox** regions;
   where the rectangle sits; how the digit sits on the **bbox** edge;
   **relations to other overlays**—distance, overlap,
   **left/right/above/below** of another **`index`**,
   whether **bbox** regions are **touching or clearly separate**.
3. **Element inside the bbox:** Describe **what is wrapped**—
   control types, visible text, icons, chrome vs page body,
   and anything salient inside that bbox. (Feeds step 5; **not** used in step 4 to accept or reject the **`index`**.)

4. **Re-compare (mandatory) — target in bbox only:** Does the step-1 target (from **`Next:`**, located in step 1) **lie inside** this candidate **`index`**’s bbox on the chosen frame?
   **Match** = the intended widget is **inside** this **bbox** (even if the **bbox** also wraps other UI). **Do not** in step 4 reject a **bbox** for being “too fat” or for containing extra controls—that belongs to **step 5** only.
   **Mismatch** = the target is **not** inside this **bbox** (wrong region, wrong **bbox**, no overlap with the aim).
   This step **only** answers **which overlay digit** is a valid **container** for the target—it **does not** count inner elements and **does not** choose **coordinates**.
   If **mismatch**, **reject** this **`index`**, **go back to step 2**, pick another candidate, repeat steps 3–4 until **match** or you exhaust plausible **`index`** values.
   When the loop ends, carry forward either **one matched `index`** (then step 5) or **no match**
   (then only step 6 may choose **coordinates**)—step 4 itself never outputs “use coordinates”.

5. **BBox wrap count (step 5 only):** Run **only** when step 4 **matched**.
   This step **only** counts how many distinct targets sit **inside** that bbox—it **does not** repeat step 4’s inclusion check.
   Count **one** vs **multiple** distinct targets **inside the bbox**
   (label + input = **two**; several icons = **multiple**; same semantic “row” still **multiple** if several pieces share one **bbox**).
   Overlay **`index`** uses the **region center**—multiple sub-targets make numbered aiming unsafe.
6. **Conclude (aiming method, last):** **Step 6** **only** maps steps 4–5 to a tool choice; **step 4** never chooses **coordinates**.
   - Step 4 **matched** **and** step 5 **single** → **`index`** for that pair.
   - Step 4 **matched** **and** step 5 **multiple** → **coordinates** (or another tool) for the exact sub-target—**not** **`index`** for that fat bbox.
   - After step-2–4 re-search, **no** candidate **`index`** gets step-4 **match** (target never inside any tried **bbox**) → **coordinates** (or another tactic) for the step-1 target on **`[Screen after action]`** scale per the inject.
   - Step 4 **mismatch** on the current candidate: handled **inside** step 4 by **return to step 2**; **do not** conclude **coordinates** there.

**Positive example** — Single **bbox**, single target: `4 match` + `5 single` → `6 index`.

```text
Location:
1 [Screen after action]: row-title overflow ⋯ chip, just right of title.
2 [Zoom pointer after action] index 4: **background color** behind **4** matches magenta **bbox** border color; tight **bbox** right of title; index-4 **bbox** not touching index-3 **bbox**.
3 Inside bbox: ⋯ overflow chip only.
4 match: ⋯ inside index 4 bbox.
5 single.
6 index 4
```

**Positive example** — URL still `4 match` inside fat strip; `5 multiple` → `6 coordinates`.

```text
Location:
1 [Screen after action]: URL field only (toolbar strip).
2 [Annotated after action] index 12: **background color** behind **12** matches orange **bbox** border color; wide toolbar strip under tabs.
3 Inside bbox: URL field, star button, extension icons — multiple distinct hit targets.
4 match: URL inside index 12 bbox.
5 multiple.
6 coordinates: URL field center; not index 12
```

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
