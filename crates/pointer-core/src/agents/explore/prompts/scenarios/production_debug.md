## Scenario: production_debug

Trace **backward from the symptom anchor** (error text, log line, failing handler)—not a repo-wide inventory.

**Mandatory:** Summary, Key files, Evidence with path+line per hop, **Execution paths** ≤5 hops.

**Optional:** Registration when config/flags involved.

**Skip:** full Surfaces / Impact map unless fix will change wire strings or cross client-server.

Modifier **`+cross_module`:** add Surfaces subsection to Impact map when fix scope expands.
