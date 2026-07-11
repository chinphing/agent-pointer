### Scenario: captcha

Judge whether captcha interaction progressed.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

| Outcome | `action_result` | `failure_cause` |
| --- | --- | --- |
| Challenge cleared or next step shown | `pass` | — |
| Error message visible | `fail` | `wrong_operation` |
| Same challenge, no change | `fail` | `precision_miss` |

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. Before/after captcha state — cleared, next step, error, or unchanged.
2. Pass or fail + `failure_cause`.
3. Optional: loading if challenge still animating.

**Forbidden:** numbered CoT templates; JSON.
