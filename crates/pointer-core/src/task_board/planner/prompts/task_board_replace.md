Replace the full board when scope or milestone list must change.

**When to call**
- Board exists and user changed requirements materially.

**When not to call**
- Board empty (use `task_board_init`).
- User only said "continue" on same scope.

**Required**
- `items` — full new milestone array.

**After success**
- Do not call more tools on subsequent rounds.
