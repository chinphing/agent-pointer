## Scenario router

Read the first line of **`instruction`**: **`Scenario: <id>`** or **`Scenario: <primary>+<modifier>`**.

Pick the matching playbook under **Scenario playbooks** below. If absent, use **standard** flow.

**Dependency layers** (reachability tasks): tag Evidence with **Compile**, **Type reuse**, **Runtime call**, **Test-only**. **Import ≠ call.**

**Default inventory prune:** skip dependency install dirs, build output, VCS metadata unless the task names them—record in **Coverage**.
