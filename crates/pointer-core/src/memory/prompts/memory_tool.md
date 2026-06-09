### `memory`

Save durable information to persistent memory that survives across sessions.
Memory is injected into future turns as a frozen snapshot; keep entries compact.

**When to save (proactively):**

- User corrects you or says to remember something
- User shares preferences, role, timezone, or communication style
- You learn stable environment or project conventions

**Targets:**

- `memory` — agent notes (environment, conventions, lessons learned)
- `user` — user profile (preferences, identity, expectations)

**Actions:** `add`, `replace` (match `old_text` substring), `remove` (match `old_text`).

Do **not** save task progress, session TODOs, or one-off debugging context — use the task board or conversation history instead.
