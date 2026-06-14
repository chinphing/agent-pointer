### Scenario: refactor

**Classify** when behavior should stay equivalent unless the user explicitly wants API change.

| Risk | Orient action |
|------|---------------|
| Rename/move within one module | Lite grep closure → optional **`single_module_fix`** explore |
| Public API / shared types / wire strings | **`cross_module_change`** explore mandatory |
| Delete code / dead paths | **`reachability_audit`** explore; verify runtime vs compile-only |

**Change:** smallest steps; one logical move per sub-goal when risk is high.

**Check:** existing tests must pass; add coverage when refactor lacks tests.
