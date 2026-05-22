## `[CUR_SCREEN]` image order

Each screenshot is preceded by its **slot label** on its own line. Use **only** the current **`[CUR_SCREEN]`** block; older turns are stripped.

**Order when a prior turn exists:** **`[Screen before action]`** → **`[Zoom pointer before action]`** → **`[Screen after action]`** → **`[Annotated after action]`** → **`[Zoom top after action]`** → **`[Zoom bottom after action]`** → **`[Zoom pointer after action]`**.

First capture omits the two **before** slots.

**Text under `[CUR_SCREEN]` (not images):** **Pointer position** + **Overlay reference bboxes** (`R: (left, top, right, bottom)` session integers).

Stage rules live in **communication** — **Part 1** (Pointer, Verify), **Part 2** (Repetition), **Part 3** (Next, Location, Recheck, Tool route).
