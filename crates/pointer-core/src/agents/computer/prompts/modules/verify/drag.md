## Verify module — drag family

Judge whether drag-and-drop produced the expected outcome.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Expected change:** Item moved, reordered, transfer started, or selection extended.

**Special rules:**
- Item at new position → **pass**.
- No move → **fail** (`precision_miss`).
- Wrong item affected → **fail** (`wrong_operation`).
- File transfer: set `loading_detected` when progress/spinner visible.

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
