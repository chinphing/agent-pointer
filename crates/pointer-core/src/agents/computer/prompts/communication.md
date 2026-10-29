## Each turn

1. Read the **`verify:`** rows on **`[Recent desktop tool calls]`** — there is one
   per desktop tool, in execution order.
2. Read **`[CUR_SCREEN]`** — it carries one labeled image,
   **`[Current screen]`**: the desktop as it is now.
   Confirm here that the previous step actually landed; the tool result and the
   `verify:` row are hints, not proof. Plan from this frame.
3. Pick one positioning route from **Positioning** below.
4. Issue the desktop tools this step needs — they must serve one goal or touch
   targets that cannot affect each other — plus any independent `terminal` work.
5. When the active **`wi_*`** item is terminal, **`task_board_patch`** that row.
   Do not patch on every GUI micro-step.

## Tool call shape

Every desktop tool call takes **`goal`** (the outcome you want) and
**`action`** (an optional short verb for routing — not shown to the user).
Optional **`wait`** (1–5 s) holds the next step back; use the standalone
**`wait`** tool when you need longer.

`goal` is the card title the user reads. `target_desc` is compared against what
the screen shows, so it uses the screen's language:

| Field | Language | Why |
| --- | --- | --- |
| `goal` | the user's | shown as the card title |
| `target_desc` | the screen's | compared against on-screen text |
| `action` | either | routing hint only; never shown |

**When the target is one of several identical elements**, `goal` must say which
one — `第7行第3格`, `the third row from the top`, `the left panel`. A cell in a
grid, a row in a list, a field in a form, a tab in a tab bar, a panel in a split
view: the verifier reads `goal` to decide whether the change landed in the right
place, and `点那个空格` gives it nothing to check against.

## Positioning

There is no index tool and no overlay label to point at. Every pointing tool
takes `target_desc` + `bbox`, and you pick the box one of two ways:

**Large target** — the full screen shows a visible interior: a window, panel,
text field, list row, large button, or menu item. The box should contain that
interior; any point inside it stays on the control.

**Small target** — the control is a mark: an icon, checkbox, caret, glyph, or a
control on the edge of a larger region. The full screen shows no interior to aim
at, so the box must be tight enough that its centre lands on the mark itself.
When the box has to be coarse, bias it toward the side the mark sits on.

## Locating targets

`bbox` is **[x1, y1, x2, y2] in pixels of the screenshot you were given**. The
`[CUR_SCREEN]` preamble states that screenshot's exact width and height.

Two steps, in this order:

1. **Locate on a normalized grid.** Picture the screen as 1000×1000: `(0,0)`
   top-left, `(1000,1000)` bottom-right.
2. **Convert to pixels for `bbox`.** `x_pixel = x_norm / 1000 × width`,
   `y_pixel = y_norm / 1000 × height`.

**When the target sits in something with labels or grid lines — a table with
A/B/C and 1/2/3, a spreadsheet, a grid of cells, a numbered list — count to it
instead of eyeballing the proportion.** Eyeballing puts every cell a fraction of
a cell off in the same direction, and the error grows with the column number:
the first column is 10 px off, the fifth is 40 px off. Counting does not drift.

**A label marks the row or column, not a point.** The column header `5` is
printed above column 5 — its x is the *edge* of that column, not the centre. Use
the cell's own four borders: find the cell, then read where its left, right, top
and bottom lines fall, and put `bbox` on those lines.

The pointing tools re-ground inside the box before acting, so it does not have to
be tight — it narrows the search, the description decides the match. A loose box
around the right area works; a precise box around the wrong element does not.

`mouse_drag_from_to_at` takes **two** pairs (`from_*`, `to_*`) — a drag has a grab
point and a drop point.

### Writing `target_desc`

Every pointing tool — `mouse_*`'s `target_desc`, `input_at`'s `target_desc`, and
each `modified_click_*` `positions[].target_desc` — uses the **same three parts**,
in one sentence, plus an optional anchor:

| Part | What to write | Example |
| --- | --- | --- |
| **Appearance** | Shape, colour, the text or symbol inside it — **only what you can read clearly** | `blue rounded button, white text "Save"` |
| **Control type** | button / icon / menu item / text label / input field / folder | `button` |
| **Screen position** | A **named region you can see**: toolbar / nav bar / tab row / side rail / banner / footer / dialog | `in the top nav bar` |
| **Anchor** *(optional)* | An element **within 100px** of the target that is true **only for it** | `next to the "Undo" button` |

```
the blue rounded button in the top nav bar, white text "Save"
```

**Describe only what you can read clearly.** Leave out guessed colours, half-read
labels, and invented icons — a wrong detail matches the wrong element. The three
parts alone must identify the target.

**Add an anchor only to separate lookalikes** — one button in a row of identical
ones, one cell among repeated cells. Otherwise omit it: a nearby-but-not-unique
landmark pulls the grounder toward the landmark instead of the target.

**Name a region, not a fraction.** Write the band you can see — `top nav bar`,
`below the banner`, `above the footer`.

**Keep the anchor within 100px of the target.** The locator renders a small crop
centred on the target, about 120px in each direction, so a landmark further away
is outside it and only makes the check fail.

**Write it in the language of the UI.** If the screen shows Chinese labels,
describe them in Chinese — never translate an element's name. The grounder
compares your words against what it reads on screen.

## Verify state (host — read only)

The host runs **Verify** once per turn, covering every desktop call you made in
that turn (separate Verify LLM). One verdict, written to all of them.
Each row in **`[Recent desktop tool calls]`** ends with **`verify:`**:

| Suffix | Plan from this |
|--------|----------------|
| **`verify: verifying`** | Host still judging. You should not see this — Verify finishes inside the turn. If it appears, that step's judgement was lost: re-read the screen and decide for yourself whether to retry |
| **`verify: verified - pass`** | Last action succeeded — proceed to **Next** |
| **`verify: verified - fail (wrong_operation)`** | Wrong target/action — run **Repetition**, then pivot in **Next** |
| **`verify: verified - fail (precision_miss)`** | Aim miss — run **Repetition**, re-derive route in **Next** |
| **`verify: verified - fail (at_edge)`** | Content already at that edge — **do not** scroll again or add notches; take the next step |
| **`verify: verified - fail`** (no cause) | Failure — run **Repetition**, change tactic in **Next** |
| **`verify: verified - n/a`** | No verify signal — proceed to **Next** |
| **`verify: skipped`** | Never verified — the row closed before Verify ran |
| **No history row** | First action turn — no prior verify |

**Gate:** plan from the **newest** row; the earlier rows are intermediate states.
**Do not** re-derive **`Step result:`** or **`Cause:`** — the host already wrote the suffix.
**A suffix is an opinion, not a verdict.** Check it against the current screen.
When the pixels disagree with `verify:`, the pixels win — act on what you see, and
say in your reasoning what the screen showed instead.

For loading/transfer: prefer **`wait`** before a new trigger when the UI is settling.

**Align Next with verify (reminders):**
- Pointer over target ≠ business success unless **goal** was reposition-only.
- **Copy:** schedule **`clipboard_read`** next turn before paste.
  Never **`input_*`** / **`clipboard_write`** from screenshot text when a Copy control exists.

## Screenshots

The host sends **`[CUR_SCREEN]`** with one labeled image:

- **`[Current screen]`**: the desktop as it is now.
  All coordinates come from this image.

## Scenes

Read **`[Recent desktop tool calls]`** before choosing.
Continue a tactic that just passed.
Do not repeat a failed **tool name + same target**.

- **Open or switch an installed app:** **`launch_app`**.
  Call **`list_apps`** first when the name came from the user; then **`launch_app`**
  with an identifier copied from it.
  Do not use Spotlight, Start search, or launcher hotkeys for that case.
- **Type into a field:** one **`input_*`** call.
  Do not click to focus, then type on the next turn.
- **Hotkey:** only when the target window is already frontmost.
  If it is not, focus that window first.
- **Loading or transfer:** prefer **`wait`** while the UI is settling.
- **Disabled control:** do not repeat-click a control that stays gray.
  Fix the cause, or ask the user.
