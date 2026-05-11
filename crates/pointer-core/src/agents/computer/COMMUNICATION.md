## `<thoughts>` (slim reasoning template)

Keep `<thoughts>` compact. Cover **four blocks** in order (plain sentences; no need for XML inside `<thoughts>`).

### 1) Action verify

Decide whether the **last** desktop action achieved its intent using **`[Screen before action]`** vs **`[Screen after action]`** (if both exist), plus **pointer/caret** position vs the intended target. Say **verified / partial / failed** and one concrete visual cue.

#### Examples for action verify

- After image shows the Save dialog closed while before had it open; pointer sits on the main canvas — report **VERIFIED** with that cue.
- After image still shows the login button enabled; before/after dialog unchanged; pointer strip shows the cursor off the button — report **FAILED** with that cue.

### 2) Action repetition

Compare the **goal** and **action** lines from **`[Recent desktop tool calls]`** (host-injected; newest at bottom). Detect if the **same intent** repeats without progress. **Do not** use overlay index numbers from those lines as proof of repetition — indices reset each annotation.

When you judge that the **same** goal+action (or equivalent intent) has repeated, state **how many consecutive times** it appears in that recent list (count from the block, oldest→newest). If the count is **more than three**, you **must** call this out and commit in `<thoughts>` to doing **something materially different** next (different control, hotkey, **`wait`**, coordinate aim, scroll, etc.) — not another nudge of the same failed move.

#### Examples for action repetition

- Recent rows differ in goal/action, or the latest screenshot clearly advanced — report repetition **OK**, not a stuck loop.
- **4** consecutive rows share the same goal/action; UI unchanged — report **STUCK**, note **>3**, and state the next turn will **do something different** (e.g. another field via hotkey or coordinates), not the same click again.

### 3) Next action

From verify + repetition, pick **one** next step. Name the **target by visible traits only** (label text, color, shape, relative place). **Base the description only on `[Screen after action]`** — the unindexed full-screen after capture. **Do not** look at **`[Annotated after action]`** (or rely on overlay numbers) when wording this block; numbers come later in **Target location**. **Do not** put overlay numbers here.

#### Examples for next action

- Focus the address field — from **`[Screen after action]`**, long pale strip under the tabs, left of the star icon; prior verify failed; repetition OK.
- Same gray “Submit” again while the form error is unchanged and recent rows show duplicate submits — must change approach, not repeat that click.
- Wording like “use index 5” or describing the target from **annotated** badges — avoid; keep **`[Screen after action]`** and traits only in this block.

### 4) Target location (indices vs coordinates)

Propose **candidate overlay numbers** only after next-action text (that text was defined from **`[Screen after action]`** only). For each candidate, cite **which image** you looked at (**`[Zoom pointer after action]`** preferred, else **`[Annotated after action]`**) and the **visible traits inside that box**. Match or reject against the next-action description.

**Reject index** and switch to **coordinate** tools when: (a) the box **covers multiple** distinct controls; or (b) the boxed region **does not match** the described target (pick another index or fall back to **`mouse:click_at`** / **`composite_action:type_text_at`**).

#### Examples for target location

- On **`[Zoom pointer after action]`**, badge 22 sits only on the three-dot control — matches the “overflow menu” described earlier; single widget — use that index.
- **`[Annotated after action]`** shows box 22 wrapping both the text field and “Go” — reject that index for a single-field aim; switch to **`composite_action:type_text_at`** with coordinates.
