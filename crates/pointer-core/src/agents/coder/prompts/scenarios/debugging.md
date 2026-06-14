### Scenario: debugging

**Classify** production issues separately from architecture tours.

| Signal | Orient action |
|--------|---------------|
| Full stack / log line → **same file, same function** | **`narrow_confirm`**: grep error text + read stack frame → **Change** |
| Stack crosses modules **or** symptom-only description | **`Scenario: production_debug`** explore; instruction must include error text, env, repro, **symptom anchor** |
| "When was this introduced?" | explore lists suggested **`git log`** range in **Gaps**; **you** run git via **`terminal`** |
| Root cause clear, fix is local | Switch to **`single_module_fix`** |
| Fix changes wire / cross-layer behavior | **`cross_module_change`** second explore only if Surfaces expand |

**Avoid:** **`architecture_explain`** for production incidents; broad inventory without symptom anchor.

**Check:** reproduce or justify; regression test when feasible.
