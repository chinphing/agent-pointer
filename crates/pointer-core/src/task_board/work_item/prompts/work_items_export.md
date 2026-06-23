Export completed work_items to a file for user delivery.

**When to call**
- Delivery milestone (`deliver_*`) is current and prerequisite batches are done.
- User asked for xlsx/csv/report file.

**When not to call**
- Batches still in progress.
- No work_items on the board.

**After success**
- Patch delivery milestone to `done`.
- Reply with `MEDIA:<path>` and one-line summary.
