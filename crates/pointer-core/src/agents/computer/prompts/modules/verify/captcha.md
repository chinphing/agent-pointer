## Verify module — captcha family

Judge whether captcha interaction progressed.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

- Challenge cleared / next step → **pass**.
- Error visible → **fail** (`wrong_operation`).
- Same challenge, no change → **fail** (`precision_miss`).

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
