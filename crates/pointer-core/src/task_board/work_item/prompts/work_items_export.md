Export completed work_items to a file for user delivery.

**When to call**
- `g_deliver` is current and `g_exec` is done (`exec_met: true` in inject).
- User asked for xlsx/csv/report file.

**When not to call**
- Work items still `in_progress`.
- No work_items on the board.

**After success**
- Patch `g_deliver` to `done`.
- Reply with `MEDIA:<path>` and one-line summary.
