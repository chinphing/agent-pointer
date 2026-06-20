## Handoff contract (explore → lead)

Final turn: write **Markdown** in assistant **`content`** only (no tools on that turn). The lead reads **`run_subagent` → `content`**.

### Minimal deliverable (always)

- **`## Summary`** — one short paragraph.
- **`## Key files`** — bullet paths.
- **`## Evidence`** — Claim / Where / Why bullets; use layer tags (`Compile`, `Runtime call`, …) for reachability tasks.
- **`## Gaps for parent`** — empty section allowed.
- **`## Coverage`** — searched, skipped (with reason), negative greps.

### Lite pack (narrow / explain-only)

When breadth is small, **Summary + Key files + Evidence** is enough. Omit empty **`## Impact map`**.

### Impact map (behavior-change / cross-module)

Include **`## Impact map`** with subsections as needed (omit empty):

- **References** — identity fan-out + grep hits.
- **Registration chain** — define → register → default → read → display (when wiring matters).
- **Readers** — each line: `` `path:Lx-Ly` `` + **`call`** / **`import`** / **`config`** (prefer call-site when read).
- **Lifecycle**, **Symmetry**, **Test & drift**, **Surfaces** — when state, locks, tests, or cross-layer readers apply.

**Lite bar:** at minimum **References + Readers + Surfaces** for cross-module work.

**Optional:** **`### Execution paths`** — see **Execution paths (when mandatory)** rules.

### Lead context (parent should paste in `context`)

Optional headings: **Lead context (trusted)** / **Already checked** / **Still unknown**.

Provenance tags: **`USER_STATED`**, **`READ_AT path:Lx-Ly`**, **`GREPPED pattern=… hits=N`**, **`Assumptions (unverified)`**.

Explore must **not** repeat greps listed under **Already checked**.

### Corrections

If the repo contradicts lead facts, add **`## Corrections to lead context`**.
