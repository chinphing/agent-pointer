### Scenario: hotkey

Judge whether the keyboard shortcut produced the expected UI effect.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

| Outcome | `action_result` | `failure_cause` |
| --- | --- | --- |
| Focus change, dialog, navigation, or app-specific effect matches goal | `pass` | — |
| Copy: no error, shortcut likely ran | `pass` | — |
| Copy/paste: clipboard outcome uncertain | `pending` | — |
| Paste: text appeared in target field | `pass` | — |
| Alt/Cmd+Tab: target app frontmost | `pass` | — |
| No visible effect | `fail` | `wrong_operation` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Before/after for the effect class implied by `goal` (focus, dialog, navigation, paste, etc.).
2. Pass, `pending`, or fail + `failure_cause`.
3. Optional: loading if applicable.

**Forbidden:** pointer analysis; numbered CoT templates; JSON.
