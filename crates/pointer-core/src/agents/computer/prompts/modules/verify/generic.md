## Verify module — generic family

Fallback verifier when no specific family matches.

Submit the result by calling **`submit_verify`** once after proof in **reasoning_content**.

**Channels:** analysis → `reasoning_content` (4–8 sentences); result → `submit_verify` only; `content` empty.

Follow pointer click verify process (concise):
1. Compare Before vs After.
2. Classify evidence.
3. Pointer rule (pointer at ≠ pass).
4. Derive `action_result`.
5. `step_summary` on pass only — in **tool call**, one sentence.
6. `loading_detected` when applicable.

**Forbidden:** other tools; prose in `content`; analysis inside tool args except allowed schema fields.
