### Scenario: hotkey

Judge whether the keyboard shortcut produced the expected UI effect.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

Read the effect class `goal` implies in both frames. Only a **difference between
the two** counts as the shortcut's effect — a dialog already open in the before
frame did not open because of this shortcut.

| Difference | `action_result` | `failure_cause` |
| --- | --- | --- |
| Focus change, dialog, navigation, or app-specific effect matches goal | `pass` | — |
| Copy: no error, shortcut likely ran | `pass` | — |
| Copy/paste: clipboard outcome uncertain | `pending` | — |
| Paste: text appeared in the target field | `pass` | — |
| Alt/Cmd+Tab: target app frontmost | `pass` | — |
| No change anywhere | `fail` | `wrong_operation` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. The effect area in before, then in after — what differs.
2. Pass, `pending`, or fail + `failure_cause`.
3. Optional: loading if applicable.

**Forbidden:** numbered CoT templates; JSON.
