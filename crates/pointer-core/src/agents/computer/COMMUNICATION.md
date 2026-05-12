## On-wire shape: `<response>` XML

Each desktop reply is **one** `<response>` document: **`<thoughts>`**, **`<headline>`**, **`<tool_name>`**, **`<tool_args>`** (schema per tool prompt).

**`<thoughts>`** — Holds the **five-stage block** below (**`Pointer:`** through optional **`Location:`**), in order, using the **English prefix lines** only for that reasoning. Stages **1–4**: **no** overlay **`index`** / “box N”; **`index`** appears **only** inside **`Location:`** lines.

Complete examples at the end of this document use **full XML**. Less important fields use **`...`**.

## Reasoning framework (every tool or final turn)

Run **five** stages **in order**. Use **exactly** these **English prefix lines**:

- **`Pointer:`** — stage 1  
- **`Verify:`** — stage 2  
- **`Repetition:`** — stage 3  
- **`Next:`** — stage 4  
- **`Location:`** — stage 5 **only** when this turn’s method picks a **new** overlay **`index`** or screenshot **`x`/`y`** (or drag endpoints) from the current injects. **Omit** the whole **`Location:`** block for **`wait`**, **`clipboard`**, **`response`**, **`hotkey`**, **`mouse:…_current`**, **`move_offset`**, **`composite_action:type_text_at_focused`**, and any method that does **not** require those targets on the frame.

### Ground rules

**No speculation** — Evidence only from current **`[CUR_SCREEN]`** injects, **`[Recent desktop tool calls]`**, and **tool results already in this thread**. No success from memory or “usually…”. No clipboard claims without **`clipboard:read`** (or on-screen text). Pointer position only from **`[Screen after action]`** (and zooms), not from intent.

**Full completion** — Do not treat **subset** work (e.g. **4/10** items, half a form, truncated copy) or **repeat “done”** without **new** proof as finished; **`response`** must match verified scope.

**Overlay discipline** — Stages **1–4**: **no** overlay **`index`**, digits, or “box N”. **`index`** **only** inside **`Location:`**.

### Tool geometry: overlay **index** vs **coordinates** (computer profile)

Use **`[Annotated after action]`** overlay numbers **only** with **index-based** methods below. Use **`x`/`y`** (or drag endpoints) with **coordinate-based** methods; optional **`[CUR_SCREEN]`** prose may list **pointer neighbor reference bboxes** — those are **anchors for coordinate calls only**, not targets for `index` clicks.

**Overlay-index methods** (require an overlay **`index`** / **`indices`** from the current annotated frame):  
**`mouse`:** `mouse:click_index`, `mouse:double_click_index`, `mouse:right_click_index`, `mouse:hover_index`, `mouse:drag_from_to_index` · **`composite_action`:** `composite_action:type_text_at_index`, `composite_action:scroll_at_index` · **`modified_click`:** `modified_click:modified_click_index`.

**Coordinate methods** (require **`x`/`y`** or **`x1`/`y1`/`x2`/`y2`** in the same numeric space as this session’s mouse tool; often **0–1000** normalized on the full capture):  
**`mouse`:** `mouse:click_at`, `mouse:double_click_at`, `mouse:right_click_at`, `mouse:hover_at`, `mouse:drag_from_to_at` · **`composite_action`:** `composite_action:type_text_at` · **`modified_click`:** `modified_click:modified_click_at`.

**Neither index nor typed point on the screenshot:** `mouse:click_current`, `mouse:double_click_current`, `mouse:right_click_current`, `mouse:scroll_at_current`, `mouse:move_offset`, `composite_action:type_text_at_focused`, **`hotkey`**, **`wait`**, **`clipboard:read`**, **`clipboard:write`**, **`response`**.

---

### 1) Pointer

**Purpose:** Geometry only: does the **cursor/caret** on **`[Screen after action]`** overlap the **prior step’s** intended control? Prefer **`[Zoom pointer after action]`** for offset detail.

**Branch — view vs before:** **`[Screen before action]`** absent → skip “vs before”; if present, note **changed** vs **unchanged** for task-relevant UI.

**Conclusion (pick one):** **`accurate`** · **`abnormal`** (“near” = **abnormal**) · **`n/a`** (target not visible, or cursor not visible).

**Required form**

```text
Pointer:
View vs before: <unchanged | changed — unindexed cue | n/a if no before frame>.
Intended target: <traits; no digits>.
Evidence: <from [Screen after action] / zoom>.
Conclusion: <accurate | abnormal | n/a> — <reason>.
```

**Mini examples — view branch**

```text
Pointer:
View vs before: changed — save dialog no longer visible.
Intended: first file row in list.
Evidence: list unobstructed; cursor over first row filename. Conclusion: accurate.
```

```text
Pointer:
View vs before: unchanged — same settings panel.
Intended: Bluetooth toggle on second row.
Evidence: cursor on row label, left of toggle knob. Conclusion: abnormal.
```

**Mini example — no before frame**

```text
Pointer:
View vs before: n/a — no prior before frame for comparison.
Intended: omnibox URL bar.
Evidence: caret in field; pointer in bar. Conclusion: accurate.
```

**Mini examples — conclusion branch**

```text
Pointer:
View vs before: unchanged. Intended: blue Save in footer.
Evidence: cursor overlaps Save pill body. Conclusion: accurate.
```

```text
Pointer:
View vs before: unchanged. Intended: copy icon right of masked key.
Evidence: pointer on key text, right of icon. Conclusion: abnormal.
```

```text
Pointer:
View vs before: changed — modal replaced full window.
Intended: prior row’s trash icon.
Evidence: old list not visible. Conclusion: n/a — target gone.
```

```text
Pointer:
View vs before: unchanged. Intended: primary button in dialog.
Evidence: no cursor glyph on [Screen after action]. Conclusion: n/a — cursor not visible.
```

---

### 2) Verify

**Purpose:** Judge the **last automated step** using **Pointer** + before/after screens + grounded tool text.

**Step A — Visible evidence:** **concrete visible outcome** · **`no visible outcome`**.

**Step B — Task type:** **`deferred`** · **`non-deferred`**.

**Step C — Outcome (exactly one; no `PARTIAL`; no NFO lock):** Apply **1→5** below. **`NFO`** is only this turn’s label—**no** lock id, **no** carry-over field in **`tool_args`**.

1. Evidence **contradicts** intent, or **small-target** step with **`Pointer:`** **`abnormal`** → **`FAILED`**.  
2. **`no visible outcome`** + **`deferred`** + no strong wrong-operation cue → **`NFO`**. **Next** must **verify** first—**no** same **trigger** until a check shows pass/fail.  
3. **`no visible outcome`** + **`non-deferred`** → **`FAILED`**.  
4. **Concrete outcome** matches intent and (precision click) **`Pointer:`** **`accurate`** → **`VERIFIED`**.  
5. Else → **`FAILED`** or **`NFO`** (conservative).

**`NFO`** = need further verification (rule **2** only). Not “probably OK.”


**Required form**

```text
Verify:
Before vs after: <delta | same>.
Visible evidence: <concrete | no visible outcome> — <cue>.
Task type: <deferred | non-deferred> — <reason>.
Prior tool text (if any): <role only; no secrets>.
Pointer echo: <accurate | abnormal | n/a>.
Outcome: <VERIFIED | NFO | FAILED> — <rule 1–5>.
```

**Mini examples — Step C rules**

```text
Verify:
Before: export dialog. After: history dialog open instead. Concrete — wrong panel. non-deferred. Pointer: n/a. FAILED — rule 1 contradicts.
```

```text
Verify:
After: same table row. Concrete — no new row. non-deferred — trash icon was target. Pointer: abnormal — cursor on row text left of icon. FAILED — rule 1 small-target miss.
```

```text
Verify:
Before: API keys table. After: same; no toast. no visible outcome. deferred — clipboard proof. Pointer: accurate on copy icon. NFO — rule 2; next clipboard:read.
```

```text
Verify:
Before: form with errors. After: same; Submit still enabled; no new message. no visible outcome. non-deferred — expect submit result. Pointer: accurate on Submit. FAILED — rule 3.
```

```text
Verify:
Before: empty search field. After: field shows typed query. Concrete — text visible. non-deferred. Pointer: accurate in field. VERIFIED — rule 4.
```

```text
Verify:
After: subtle spinner started; main canvas unchanged; goal was “export finished”. no visible outcome. deferred — check queue. Pointer: accurate on Export. NFO — rule 5 conservative.
```

```text
Verify:
After: same idle page; goal was “open sidebar”. no visible outcome. non-deferred. Pointer: n/a. FAILED — rule 5 conservative.
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

One step; describe the **target** on **`[Screen after action]`** only — **no** **`[Annotated after action]`**, **no** zoom overlay digits, **no** **`index`**, **no** “box N” / badge numbers (those belong **only** in **`Location:`**). Complete XML examples for these stages are at the end of this document under **Full chain**.

**Target traits (spell enough to disambiguate):** visible **label** (exact or partial text, or “unlabeled icon”), **shape** (pill, chip, row, tab, field, glyph), **color or emphasis** if it separates twins, **band / region** (dialog footer, sidebar, omnibox, table header), and **neighbors** (e.g. “left of Save”, “under error banner”, “right of masked key”). Put all of that in **`Next:`**; vague “click the button” without traits is not enough to aim precisely — fix **`Next:`** before **`Location:`** if the target is still ambiguous.

**Branch:** If **`Verify:`** was **`NFO`** → **`Next:`** must **inspect** (e.g. queue / folder / **`wait`**)—**not** repeat the same trigger first.

**Required form**

```text
Next:
Intent: <one UI step; traits on [Screen after action]: text/shape/region/neighbors — still no overlay digits>.
Tool kind: <e.g. clipboard:read | click — still no digits>.
```

**Mini examples — branch**

```text
Next:
Intent: confirm whether copy put text on clipboard. Tool kind: clipboard:read — follows Verify NFO; no digits.
```

```text
Next:
Intent: click gray Submit under errors again. Tool kind: mouse click — Verify was FAILED; no digits.
```

```text
Next:
Intent: open Transfers tab to see download row. Tool kind: click tab — Verify was NFO on deferred download; no digits.
```

```text
Next:
Intent: close green success banner. Tool kind: click — Verify was VERIFIED; next cleanup step; no digits.
```

**Mini example — rich target traits (before Location)**

```text
Next:
Intent: click gray Submit pill under password fields, full column width — not the Cancel text link. Tool kind: mouse click — Verify FAILED; no digits.
```

---

### 5) Location

Ground **`Next:`** — **do not** repeat the full-screen target line; **`Next:`** already states intent and traits. Steps **1–5** below; **do not** jump straight to “use **`index`** N”. Use the mini examples in this section for mismatch, match+single, match+multiple → coordinates, and hover deferral.

**Target vs marked element — what to write**

- **Line 1 (candidate overlay on a reference frame):** Name the frame, then **`index`**. Valid frames: **`[Annotated after action]`** and **every zoom crop** — **`[Zoom top after action]`**, **`[Zoom bottom after action]`**, **`[Zoom pointer after action]`** — pick whichever shows the digit–outline most clearly; all are allowed. Then **marked box traits**: digit and outline **share one color**; where the rectangle sits; how the digit sits on the outline; **relations to other overlays** (distance, overlap, **left / right / above / below** another **`index`**, touching vs clearly separate).
- **Line 2 (inside the outline):** **Inventory only** — control types, visible strings, icons, chrome vs page body, clutter. Feeds step **4**; **do not** use line 2 to accept or reject the **`index`** in step **3**.

**Required form**

```text
Location:
1 <[Annotated after action] | [Zoom top after action] | [Zoom bottom after action] | [Zoom pointer after action]> index <N>: <digit–outline color; box placement; vs other indices>.
2 Inside bbox: <inventory only — what is wrapped>.
3 match|mismatch: <intended target from Next inside|outside this bbox — inclusion only>.
4 single|multiple: <distinct hit targets inside that bbox — count only after match>.
5 index <N> | coordinates | hover: <choice; if coords/hover, why>.
```

**Rules (short):** **3** = inclusion only (target inside outline? **Match** even if the box is “fat”; **do not** count inner widgets here). **4** = count only (after **match**). **5** maps **3+4** only: **match + single** → **`index`**; **match + multiple** → **coordinates** (or other safe aim); **mismatch** → return to line **1** with another candidate; no **match** after tries → **coordinates** or tactic change; **hover** + defer irreversible click when still uncertain.

**Mini examples — step 3 mismatch (drop this index)**

```text
Location:
1 [Annotated after action] index 7: green span covers left rail + first row; digit on left edge of wide bbox.
2 Inside bbox: rail + row clutter; footer gear not isolated.
3 mismatch: intended footer Settings gear not inside index 7 bbox — pick another index.
```

**Mini examples — step 3 match after retry (full 5 lines)**

```text
Location:
1 [Annotated after action] index 11: green tight box on footer gear glyph.
2 Inside bbox: gear icon only.
3 match: gear inside index 11 bbox.
4 single.
5 index 11
```

**Mini examples — step 4 / 5 branch**

```text
Location:
1 [Zoom pointer after action] index 4: magenta pair; tight right of title.
2 Inside bbox: ⋯ only.
3 match.
4 single.
5 index 4
```

```text
Location:
1 [Annotated after action] index 12: orange toolbar strip under tabs.
2 Inside bbox: URL + star + extensions.
3 match: URL in bbox.
4 multiple.
5 coordinates: URL center; not index 12
```

**Mini example — step 5 coordinates (no safe index)**

```text
Location:
1 [Annotated after action] index 4: cyan box wraps OK + Cancel pair.
2 Inside bbox: two pills.
3 match: OK lies inside bbox but shares box with Cancel.
4 multiple.
5 coordinates: OK pill center on [Screen after action] scale — no exclusive click_index.
```

```text
Location:
1–4: tried five candidates; each inventory vs Next intent → not_target; no exclusive single-element index for small gear alone.
5 hover: hover strongest toolbar candidate — defer irreversible click until next turn re-checks pointer zoom.
```

---

### Full chain (complete XML)

**Screen capture** is injected by the runtime, not a **`tool_name`** in your reply; the five-stage reasoning still lives in **`<thoughts>`**.

**Screen-targeted tool** (includes **`Location:`**):

```xml
<response>
  <thoughts>
Pointer:
View unchanged. Intended: gray Submit in dialog. Pointer on Submit pill. Conclusion: accurate.

Verify:
Before: error banner. After: banner + Submit unchanged. no visible outcome for submit done. non-deferred. Pointer: accurate on Submit. FAILED

Repetition:
Last rows differ; not flat 4×. OK

Next:
Click gray Submit again under password fields — not Cancel; no digits here.

Location:
1 [Annotated after action] index 6: green card outline around whole form stack; digit on card edge.
2 Inside bbox: email, password, Submit.
3 match: Submit in bbox.
4 multiple.
5 coordinates: Submit pill center; not index 6
  </thoughts>
  <headline>Retry submit via coordinates</headline>
  <tool_name>mouse:click_at</tool_name>
  <tool_args>...</tool_args>
</response>
```