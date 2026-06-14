## Execution paths (when mandatory)

Add **`### Execution paths`** under **`## Impact map`** (or under **`## Evidence`** for explain-only tasks) **only** when one of these holds:

1. **`Scenario: architecture_explain`** or the instruction explicitly asks for flow / trace.
2. **`Scenario: reachability_audit`**, delete-code, or dead-code—and Compile vs Runtime is disputed.
3. **`Readers`** list only **`import`** / **`reexport`** with no read call-site.
4. Debug: backward chain within **2 hops** of the symptom still does not explain it.
5. **`Scenario: production_debug`** — trace **backward from the symptom anchor** (≤5 hops).

Otherwise **skip** Execution paths. Record **`Execution paths — skipped (reason)`** in **`## Coverage`**.

**Hop budget:** default **≤5**; **`architecture_explain`** may use **≤8**.

**Anti-duplication:** if **`Readers`** already cites a call-site (`path:Lx-Ly` + `call`), **do not** repeat the same hops in Execution paths.
