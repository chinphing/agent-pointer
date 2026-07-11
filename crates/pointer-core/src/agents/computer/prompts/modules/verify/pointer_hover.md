## Verify module — pointer hover family

Judge whether hover revealed the expected UI state (tooltip, popup preview, menu, highlight).

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Special rules:**
- Success: tooltip/popup/highlight visible.
- **"Pointer at ≠ pass":** cursor on target with no UI change → fail (`wrong_operation`).
- Tooltip/menu appeared → **pass**; no visible change → **fail**.

Same evidence classification as pointer click verify (Steps 1–6).

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
