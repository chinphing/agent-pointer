### Scenario: drag

Judge whether drag-and-drop produced the expected outcome.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

**Order:** goal first → pointer/end position **only on fail**.

| Outcome | `action_result` | Fail detail | `failure_cause` |
| --- | --- | --- | --- |
| Item moved, reordered, or transfer started | `pass` | — | — |
| No move | `fail` | miss drop zone | `precision_miss` |
| Wrong item affected | `fail` | wrong target | `wrong_operation` |

**`loading_detected`:** file transfer progress or spinner visible.

### Reasoning (hard rule)

Max **4 sentences**, plain prose.

1. Before/after — item moved/reordered/transfer started or not.
2. Pass → stop; fail → wrong item vs miss drop zone for `failure_cause`.
3. On fail only: brief pointer/drop-zone note if needed.
4. Optional: loading.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
