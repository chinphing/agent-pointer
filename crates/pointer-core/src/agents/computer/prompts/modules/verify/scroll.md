### Scenario: scroll

Judge whether the viewport content moved (runtime does not detect scroll — use screenshots).

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

| Outcome | `action_result` | `failure_cause` |
| --- | --- | --- |
| New rows visible, scrollbar shifted, or content offset changed | `pass` | — |
| Unchanged, loading in progress | `pass` with `loading_detected: true` | — |
| Unchanged, no loading | `fail` | `wrong_operation` |
| Wrong area scrolled | `fail` | `wrong_operation` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Before/after viewport or scrollbar delta vs scroll `goal`.
2. Pass or fail + `failure_cause`; note wrong region if applicable.
3. Optional: `loading_detected` when unchanged but spinner visible.

**Forbidden:** pointer-position analysis; numbered CoT templates; JSON.
