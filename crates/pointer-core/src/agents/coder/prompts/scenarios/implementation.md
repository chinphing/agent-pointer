### Scenario: implementation (default)

**Classify** when the user wants code shipped in this session.

| Breadth | Orient action |
|---------|---------------|
| Known path + symbol (`narrow_confirm`) | 1 grep + 1 read → **Change** |
| Single module, API may shift | **`Scenario: single_module_fix`** explore **or** lite local grep |
| Cross-module / wire / config / state | **`Scenario: cross_module_change`** explore + **task_board_init** |
| New feature spanning layers | **`Scenario: cross_module_change`** or **`spec_map`** if spec-driven |

**Skip explore** only when edit site and readers are already proven with line evidence.

**Check:** G3 on each sub-goal; integration optional unless CI-sensitive.
