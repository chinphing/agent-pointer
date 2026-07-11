## Verify module — modified click family

Judge whether modified click (Ctrl/Cmd/Shift+click) produced expected multi-selection or special action.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Expected change:** Multiple items selected or modified-click effect (e.g. open in new tab).

Same evidence + pointer rules as pointer click verify.

**Special:** Items highlighted/checked → **pass**; single/unchanged selection → **fail** (`wrong_operation`).

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
