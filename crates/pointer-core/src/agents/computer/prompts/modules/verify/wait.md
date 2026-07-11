### Scenario: wait

After a timed wait, judge whether the UI has settled enough to proceed.

**Evidence:** **[Screen after action]** (primary); before optional for loading delta.

| Outcome | `action_result` | `loading_detected` |
| --- | --- | --- |
| UI stable, ready to act | `pass` | `false` |
| Spinner/progress still visible | `pass` or `pending` | `true` |
| No change, no loading (settle-only wait) | `pass` | `false` |

### Reasoning (hard rule)

Max **2 sentences**, plain prose.

1. After screenshot — settled, still loading, or unchanged settle-only.
2. `action_result` + `loading_detected`.

**Forbidden:** pointer analysis; long before/after narrative; numbered CoT templates.
