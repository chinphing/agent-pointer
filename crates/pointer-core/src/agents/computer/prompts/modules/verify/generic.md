### Scenario: generic

Fallback for tool names that map to no specific family. Judge only what
`goal` asked for — an unknown tool may have no landing point, no cursor, and no
selection model, so do not assume any of them.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

| Goal evidence | `action_result` | `failure_cause` |
| --- | --- | --- |
| The change `goal` describes | `pass` | — |
| A change, but not the one `goal` describes | `fail` | `wrong_operation` |
| No change anywhere | `fail` | `wrong_operation` |

**`step_summary`:** on `pass`, name the change — `panel expanded, three rows
visible`. On `fail`, name the one fact that decided it.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** numbered CoT templates; JSON; repeating tool args.
