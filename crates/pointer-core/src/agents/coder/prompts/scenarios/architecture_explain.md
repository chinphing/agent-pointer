### Scenario: architecture_explain

**Classify** when the user asks to explain **architecture**, system structure,
module boundaries, or end-to-end request / data flow — and is **not** asking
to implement in this turn.

**Orient:** When the map is not already in context, delegate explore with
**`Scenario: architecture_explain`** (Summary, Key files, Execution paths).
Reuse that handoff; do not re-walk the same hops in the lead thread.

**Deliver (SVG first):**
- Emit **one** fenced **`svg`** block in assistant **content** for the
  primary architecture or flow overview (shared SVG rules: fixed `viewBox`,
  short labels, clear gaps).
- Prefer that SVG over Markdown lists, tables, or ASCII for the overview.
- Keep a short prose summary under the diagram (layers / key hops only).
- Do **not** use Mermaid or ASCII art as the main overview.
- Do **not** write a `.svg` file and deliver it with **`MEDIA:`** for the
  overview — that becomes a file chip, not an inline diagram. Use **`MEDIA:`**
  for SVG only when the user explicitly asks for a downloadable file.

**Forbidden:** **Change** (`file_edit` / `file_write`) unless the user also
asks to implement.
