# Terminal interactive input (SSH / PTY)

When the UI provides terminal input hooks, Pointer can show a modal for
interactive prompts instead of requiring a host TTY.

## Paths

| Scenario | How input is collected |
| --- | --- |
| SSH / SCP / SFTP on **Unix** | Temporary **PTY** for host-key `yes/no`; **`SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force`** for password / passphrase |
| SSH on **Windows** (or askpass unavailable) | PTY + output heuristics; after the user already answered one prompt (e.g. host-key trust), a short-idle secret modal if `password:` is not clearly echoed |
| `sudo` / `expect` | Temporary PTY; heuristics for prompts |
| Non-PTY pipe with SSH (Unix) | `SSH_ASKPASS` only (no TTY for host-key confirm) |

## Timing

- Prompt heuristics run after ~**800ms** of idle output (`PROMPT_DETECT_IDLE_MS`), not the full command `timeoutMs`.
- Full `timeoutMs` still governs killing non-interactive pipe commands with no output.
- Do **not** force an SSH password modal on first connect with no prior input and no clear `password:` text (avoids false prompts for key auth / hanging connects).

## Related code

- `crates/pointer-core/src/tools/terminal.rs` — main loop
- `terminal_prompt.rs` — heuristics / post-interactive fallback
- `terminal_askpass.rs` — Unix askpass bridge
- `terminal_pty.rs` — PTY spawn and env
