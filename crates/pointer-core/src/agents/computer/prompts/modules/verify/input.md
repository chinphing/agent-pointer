### Scenario: text input

Judge whether text was entered into the target field.

**Evidence:** **[Screen before action]** vs **[Screen after action]** — field
content and focus.

### Step 1 — what did the field hold before, and what does it hold now?

Read the target field in both frames. Only a **difference between the two**
counts as entered text — characters already present in the before frame were
not typed by this action. `clear_first` means the before content should be gone.

### Step 2 — does the difference match what `goal` asked for?

| Difference | `action_result` | `failure_cause` |
| --- | --- | --- |
| The expected text, in the field `goal` names | `pass` | — |
| Text went into a different field, or replaced the wrong content | `fail` | `wrong_operation` |
| Nothing changed, field still unfocused | `fail` | `wrong_operation` |
| Nothing changed, cursor was off the field | `fail` | `precision_miss` |

If `auto_enter`: form may have submitted — pass when navigation, results, or
dialog close is a **difference between the two frames**.

**`loading_detected`:** post-submit spinner or page load in progress.

### Reasoning (hard rule)

Max **3 sentences**, plain prose — no numbered lists or section headers.

1. Field content in before, then in after — what differs.
2. Pass or fail + `failure_cause` when fail.
3. Optional: loading after auto-submit.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
