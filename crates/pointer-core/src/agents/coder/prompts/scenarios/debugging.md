### Scenario: debugging

**Classify** production issues separately from architecture tours.

| Signal | Orient action |
|--------|---------------|
| Full stack / log line → **same file, same function**; **no** persist/stream/Platform trigger | **`narrow_confirm`**: grep error text + read stack frame → **Change** |
| Lost after restart/reload, stream timing, Platform API | **Persistence checklist** (Routine) + multi-file trace or **`explore`** — **not** narrow confirm |
| Stack crosses modules **or** symptom-only description | **`Scenario: production_debug`** explore; instruction must include error text, env, repro, **symptom anchor** |
| "When was this introduced?" | explore lists suggested **`git log`** range in **Gaps**; **you** run git via **`terminal`** |
| Root cause clear, fix is local | Switch to **`single_module_fix`** |
| Fix changes wire / cross-layer behavior | **`cross_module_change`** second explore only if Surfaces expand |

**Avoid:** **`architecture_explain`** for production incidents; broad inventory without symptom anchor.

**Systematic debugging (before Change):** complete each phase in order — do **not** propose fixes until Phase 1 is done.

1. **Root cause** — read errors and stack traces fully; reproduce consistently or gather more data; check recent changes (`git diff`, config, deps).
2. **Hypothesis** — one likely cause at a time; add minimal instrumentation at component boundaries when the path crosses modules.
3. **Minimal fix** — smallest change that addresses the **root cause**, not the symptom alone; one hypothesis per edit when possible.
4. **Prove** — run targeted tests via **`terminal`**; add a regression test when feasible; apply **G3 evidence gate** before **Deliver**.

**Check:** reproduce or justify; regression test when feasible; prefer red-before-fix / green-after-fix when you add a new test.
