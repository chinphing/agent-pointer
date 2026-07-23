## Task board

Multi-step work is tracked with **`task_board`**, not by pasting the full plan only into assistant text or reasoning.

**User-visible replies** go in assistant **`content`**. **`task_board`** holds milestones in **`global_milestones`**.

### Init gate

Follow injected **`[TASK_BOARD_HINT]`** when present — **default skip `task_board_init`**.
Same gate for lead and sub-agents.

**Skip init** (no board) when any apply:

- 1–2 file narrow fix / typo / lint-only
- explore handoff scenario is **`narrow_confirm`** or **`single_module_*`**
- one-shot outcome the user can verify from **Deliver** alone

**Init** only when the hint gate matches (≥2 signals; coder **≥3 files** or **cross-module**,
recoverable workflow, auditable batch, high branch risk).

### When you do init — row shape

Use **3–8 task-specific** titles tied to this goal
(e.g. "Locate empty-response path", "Raise vision max_tokens", "Add truncation test").

**Do not** create a generic **Recon / Implement / Verify** (or Unit tests) three-row board
for narrow work — that pattern is reserved for **cross-module / multi-file** jobs only.

If you need a verification milestone on a justified board, set **`done_when`** to the
exact command, and on **`done`** put **`remark`**: `<command> — <outcome>` or `SKIP: <reason>`.

After explore on a **justified** board: patch the recon-style row **`done`** with a one-line
summary + key paths — not the full handoff.

### Without a board

**`narrow_confirm`** and other skip cases still follow **G3 evidence gate** in **Routine workflow** —
no board does **not** mean no tests.

### Patch discipline

- Status changes → **`task_board_patch`** with **`global_milestones`** (one row) in the **same turn**.
- Treat **`[TASK_BOARD]`** as authoritative snapshot.
- **`done`** needs repeatable evidence in **`remark`** (command output or explore summary)—not "looks good".
- Edit rows → **`done`** only after edits + **`read_lints`** when you touched code.
- Test / verify-titled rows → **`done`** only after a **`terminal`** run this session;
  **`remark`** = command + pass/fail (or explicit skip reason copied to **Deliver**).
- All rows **`done`** / **`cancelled`** → **`task_board_finalize`** + **Deliver** same turn —
  after verify evidence or skip is stated in **Deliver**.

Each row keeps **`plan`**, **`done_when`**, optional **`remark`** when `done`
(see **`task_board`** tool doc in **Tools**).

**Do not** paste explore **`## Impact map`** into board **`plan`** fields.
