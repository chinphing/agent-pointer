### Scenario: implementation (default)

**Classify** when the user wants code shipped in this session.

| Breadth | Orient action |
|---------|---------------|
| Known path + symbol; **no** persist/stream/Platform trigger (`narrow_confirm`) | 1 grep + 1 read → **Change** |
| Persist / reload / stream timing / Platform API | Multi-file trace or **`Scenario: cross_module_change`** explore — narrow confirm **off** |
| Single module, API may shift | **`Scenario: single_module_fix`** explore **or** parallel local grep |
| Cross-module / wire / config / shared state | **`Scenario: cross_module_change`** explore |
| New feature spanning layers | **`Scenario: cross_module_change`** or **`spec_map`** if spec-driven |
| User Skill under `~/.pointer/skills/` | **`Scenario: skill_change`** (not this playbook) |

**Skip explore** only when edit site and readers are already proven with line evidence **and** no high-breadth trigger applies.

**Check:** G3 on each sub-goal — **`terminal`** test after logic edits; integration optional unless CI-sensitive.

**Deliver:** do not finish until **Check** ran (or skip reason is in **`content`**).
With a board, complete its outcome row before **`finalize`**.
