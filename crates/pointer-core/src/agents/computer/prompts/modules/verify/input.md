### Scenario: text input

Judge whether text was entered into the target field.

**Evidence:** **[Screen before action]** vs **[Screen after action]** — field content and focus.

| Outcome | `action_result` | `failure_cause` |
| --- | --- | --- |
| Expected text visible in field | `pass` | — |
| Empty or wrong text | `fail` | `wrong_operation` or `precision_miss` |
| Field not focused, no text | `fail` | `wrong_operation` |

If `auto_enter`: form may have submitted — pass when navigation, results, or dialog close matches goal.

**`loading_detected`:** post-submit spinner or page load in progress.

### Reasoning (hard rule)

Max **3 sentences**, plain prose — no numbered lists or section headers.

1. Before/after field text and focus vs expected input from `goal`.
2. Pass or fail + `failure_cause` when fail — **do not** analyze pointer.
3. Optional: loading after auto-submit.

**Forbidden:** pointer-position analysis; numbered CoT templates; JSON; repeating tool args.
