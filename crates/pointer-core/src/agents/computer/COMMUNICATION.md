## `<thoughts>` (slim)

Four blocks in order, plain sentences. **Verify**, **repetition**, and **Next action**: never use overlay **`index`**, digits, or “box N” / badge-only wording (indices reset; only **Target location** picks **`index`**).

### 1) Action verify

Last step vs intent: **`[Screen before action]`** vs **`[Screen after action]`** (if before exists) + pointer/caret vs target → **verified / partial / failed** + one **unindexed** cue (text/dialog/focus/scroll vs a **labeled** control).

**Examples:** After: Save dialog gone, before had it open; pointer on canvas → **VERIFIED**. After: login button still enabled, dialog unchanged, pointer off the button → **FAILED**.

**Bad → good:** “index 14 unchanged” / “badge 9 only” → “**failed**: red Sign-in banner unchanged; pointer on empty sidebar.”

### 2) Action repetition

**`[Recent desktop tool calls]`** (oldest→newest, bottom=newest): same **goal+action** (or same intent) without UI progress? Count **consecutive** matches. **>3** → **STUCK**, commit to a **different** tactic next. Sameness = **goal/action text**, not index.

**Examples:** Rows differ or screenshot clearly advanced → repetition **OK**. **4** consecutive same goal/action, UI flat → **STUCK**, note **>3**, next step different (hotkey/coords), not same click.

**Bad → good:** “clicked index 8 thrice” / “both box 22” → “**STUCK**: 4 rows same `goal|action`; after screen unchanged — next coords or hotkey.”

### 3) Next action

One step; target = **traits on `[Screen after action]`** only (label/shape/place). **No** **`[Annotated after action]`** in this block; **no** zoom overlay digits, **no** **`index`**, **no** “box N” / “badge K” / “click N” — those belong only in **Target location**.

**Examples:** Focus address bar — long pale strip under tabs, left of star; prior verify failed; repetition OK. Gray “Submit” again, form error unchanged, duplicate rows → change approach, not repeat. Avoid “index 5” / “use 12” / describing the sole target from annotated numbers only.

### 4) Target location

After block 3, in order—**do not** jump straight to “use **`index`** N”:

1. **Overlay + frame (first):** name the frame (**`[Zoom pointer after action]`** preferred, else **`[Annotated after action]`**), the overlay **`index`** (digit), then the **box’s own traits**: outline and digit **share the same color**; where the rectangle sits on screen; how the digit sits on the outline; **relations to other overlays**—distance, overlap, **left/right/above/below** of another **`index`**, whether boxes are **touching or clearly separate**.
2. **Element inside the box (second):** only after step 1, describe **what is wrapped**—control type, visible label text, icon shape, chrome vs page body, and anything else salient inside that outline.
3. **Compare:** line up steps 1–2 with block 3’s target traits on **`[Screen after action]`**—same widget, same role, same place; note any **mismatch** (one box spans several controls, wrong region of the window).
4. **Conclude:** only then choose **`index`**, or reject and use **coordinates** / another tool if the box does not tightly match.

**Examples (good rhythm):** “**`[Zoom pointer after action]`**, **`index`** 4: **magenta** outline + **magenta** ‘4’ hugging the top-left corner of a small rectangle **tight to the right** of the row title text, **not touching** **`index`** 3’s wider bar—**inside**, only the gray **⋯** chip → matches block 3 ‘overflow beside that row title’ → **`index`** 4.” — “**`[Annotated after action]`**, **`index`** 9: **cyan** pair, box **alone in the footer band**, **below** the form fields and **left of** **`index`** 10’s outline (no overlap)—**inside**, single blue **Send** pill → matches block 3 ‘primary send’ → **`index`** 9.”

**Examples (bad → good):** “**`index`** 12 for the address bar” (no geometry) → “**`[Annotated after action]`**, **`index`** 12: **orange** box **one wide strip** under tabs, **contiguous** with **`index`** 11’s left edge (same toolbar row)—**inside**, URL field **plus** star **plus** extension icons; block 3 asked only the **URL field** → **reject** **`index`**, coords.” — Rushed: “**`index`** 7” → “**`index`** 7: **green** rectangle **covers** the whole left rail **and** the first list row, **`index`** 8 sits **inside** that same green span—ambiguous; block 3 wanted **Settings** in the **footer** → **mismatch**, pick another **`index`** or coords.”
