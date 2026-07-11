## Verify module — scroll family

Judge whether the scroll action moved the viewport content.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Expected change:** New rows visible, scrollbar thumb shifted, content offset changed.

**Special rules:**
- Runtime does not detect scroll — compare before vs after screenshots.
- New content visible → **pass**.
- Unchanged + loading → `loading_detected: true`.
- Unchanged, no loading → **fail**.
- Wrong area scrolled → **fail** (`wrong_operation`).

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
