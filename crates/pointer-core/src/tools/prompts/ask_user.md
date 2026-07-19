### `ask_user`

Ask the user to choose when required information
cannot be inferred safely.
The tool pauses the current run until the user
submits a choice.

- Keep questions concise and decision-oriented.
- Provide 2–6 mutually clear options.
- Use `multi_select` only when several choices
  may be combined.
- Do not use this tool for questions answerable
  from the workspace.

**When to use**

- Consent before delegating desktop control
  (`computer`): call this tool — do **not** ask
  consent in plain assistant text.
- Irreversible, security-sensitive, or
  product-ambiguous choices that need a clear pick.

**When not to use**

- The user already clearly asked you to control
  the desktop for **this** task (that is consent).
- Questions one workspace tool call can answer.

**Computer-consent options (example shape)**

- Allow desktop control for this task
- Instructions only (no desktop control)
- Cancel
