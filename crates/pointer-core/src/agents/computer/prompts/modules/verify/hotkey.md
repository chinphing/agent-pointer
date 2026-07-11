## Verify module — hotkey family

Judge whether the keyboard shortcut produced the expected UI effect.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

**Expected change:** Focus change, dialog, clipboard op, navigation, or app-specific effect.

**Special rules:**
- Copy: **pass** when no error and shortcut likely ran; uncertain clipboard → **pending**.
- Paste: pasted text in target field.
- Alt/Cmd+Tab: target app frontmost.
- No visible effect → **fail**.

Same evidence classification as pointer click verify.

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
