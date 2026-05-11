## Desktop vision frames (`[CUR_SCREEN]`)

Each turn you receive ordered images under the host label **`[CUR_SCREEN]`** (exact slot names appear in the first line of that message). Earlier turns’ desktop images are removed from history; rely only on the latest inject.

### Full-screen raw captures

- **`[Screen before action]`** (when present) — JPEG of the desktop **before** the most recent automated action. Use it with **`[Screen after action]`** to see what changed (new windows, focus moves, typed text, scroll position).
- **`[Screen after action]`** — JPEG of the desktop **after** that action. This is the current full-screen truth for layout and global context. A **synthetic pointer** and **caret hint** may be drawn on this frame (and on the annotated image) so you know where input was aimed. In `<thoughts>`, describe the **Next action** target **only** from this frame (no overlay numbers); use **`[Annotated after action]`** later when mapping to **`index`** in the **Target location** step.

### Annotated (indexed) view

- **`[Annotated after action]`** — Same **after-action** moment as **`[Screen after action]`**, but with **numbered overlay boxes** from the annotation service. Use these integers as **`index`** arguments for **`mouse:…`**, **`composite_action:…`**, **`modified_click:…`** when a single box tightly matches your target. Numbers are **not stable across turns**; re-read them every round.

### Zoom strips (after-action only)

All zooms are crops of the **after-action** desktop (aligned with **`[Screen after action]`** / **`[Annotated after action]`**), not the before frame.

- **`[Zoom top after action]`** — Magnified **top** strip (e.g. menu bar / window title region). Use to read small chrome, app name, or controls along the top edge.
- **`[Zoom bottom after action]`** — Magnified **bottom** strip (e.g. dock / taskbar). Use for launcher icons and status UI there.
- **`[Zoom pointer after action]`** — A magnified patch around the **mouse position at capture time**. Prefer this when checking **what is under the pointer**, fine structure next to the cursor, or ambiguous targets near the click point.

When choosing an overlay index, prefer **`[Zoom pointer after action]`** for local detail, then **`[Annotated after action]`** for global numbering. State **which frame** you used when you describe evidence (so reasoning stays tied to pixels, not free text).
