## Verify module — input family

Judge whether text was entered successfully into the target field.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Expected change:** Target field shows typed text; cursor in field.
**If auto_enter:** form may have submitted (new page, results, dialog closed).

**Special rules:**
- Expected text visible → **pass**.
- Empty/wrong text → **fail** (`wrong_operation` or `precision_miss`).
- Field not focused, no text → **fail**.
- Watch loading after auto-submit.

Same evidence + pointer rules as pointer click verify.

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
