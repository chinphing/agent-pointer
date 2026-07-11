### Scenario: clipboard

Judge whether clipboard content matches the operation `goal`.

**Primary evidence:** **[Tool result]** — not screenshot bytes.

| Source | Read actual |
| --- | --- |
| `clipboard_read` | Text after `Clipboard text:` |
| `clipboard_write` | `Copied N characters` vs intended `text` |

**Supporting only:** **[Screen before action]** / **[Screen after action]** (toast, field) — must not contradict [Tool result].

| Outcome | `action_result` | `failure_cause` |
| --- | --- | --- |
| [Tool result] matches goal | `pass` | — |
| Empty, wrong, error, unreadable | `fail` | `wrong_operation` |

**`loading_detected`:** always `false`.

**Host shortcut (you are not invoked):** empty read, zero-char write, or tool error.

Do not pass on UI toast alone without matching [Tool result].

### Reasoning (hard rule)

Max **3 sentences**, plain prose — no numbered lists or section headers.

1. Quote [Tool result] vs goal — match or not.
2. Pass or fail; cite screenshot only if it supports or contradicts tool text.
3. **Do not** analyze pointer or before/after as primary evidence.

**Forbidden:** screenshot-first judgment; pointer analysis; numbered CoT templates; JSON.
