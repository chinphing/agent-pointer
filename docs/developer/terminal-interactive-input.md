# Terminal interactive input (SSH / PTY)

When the UI provides terminal input hooks, Pointer can show a modal for
interactive prompts instead of requiring a host TTY.

## Live output vs input modal

| UI | When it appears |
| --- | --- |
| **Terminal live output** (`TerminalLiveOutputModal`) | Manual only: tool row **查看** after the command has run **≥5s** and has output. Does **not** auto-open. Place **查看** immediately after the status text (left cluster), not at the row’s far right — do not put `flex-1` on `.tool-call-trigger` or it pushes the button away. |
| **Terminal input** (`TerminalInputModal`) | Auto-opens on `terminal_needs_input` (password / stdin / host-key). Unchanged by the live-output UX. |

Password / elevation stdin prompts always take priority: the live output modal is
dismissed and hidden while an input request is active.

Background `terminal` (`blockUntilMs` set) does **not** open the input modal.
Those commands run without interactive stdin; use foreground (omit `blockUntilMs`)
for SSH / sudo prompts. Elevated + `blockUntilMs` is rejected.

## Paths

| Scenario | How input is collected |
| --- | --- |
| SSH / SCP / SFTP on **Unix** | Temporary **PTY** for host-key `yes/no`; **`SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force`** for password / passphrase |
| SSH on **Windows** (or askpass unavailable) | PTY + output heuristics; after the user already answered one prompt (e.g. host-key trust), a short-idle secret modal if `password:` is not clearly echoed |
| `sudo` / `expect` | Temporary PTY; heuristics for prompts |
| Non-PTY pipe with SSH (Unix) | `SSH_ASKPASS` only (no TTY for host-key confirm) |

## Timing

- Prompt heuristics run after ~**800ms** of idle output (`PROMPT_DETECT_IDLE_MS`), not the full command `timeoutMs`.
- A trailing `:` is treated as a prompt only for a **short** line (`Username:`). Section banners (`=== …:`) and long echoed labels do **not** open the input modal.
- Full `timeoutMs` still governs killing non-interactive pipe commands with no output.
- Idle timeout (`timeoutMs`): user setting **空闲（秒）** is the default when omitted (1–86400, default 30). The tool arg may raise or lower it (hard cap 86400s).
- Wall clock (`maxWallMs`) is capped by user setting **最长运行（小时）** (1–10000, default 24). The tool arg may only lower it.
- Do **not** force an SSH password modal on first connect with no prior input and no clear `password:` text (avoids false prompts for key auth / hanging connects).

## Agent guidance

`terminal` prompt: in-app password/host-key modals exist — run `ssh` /
`ssh-copy-id` / `scp` yourself and wait; never put secrets in tool args.

## Related code

- `crates/pointer-core/src/tools/terminal.rs` — main loop
- `terminal_prompt.rs` — heuristics / post-interactive fallback
- `terminal_askpass.rs` — Unix askpass bridge
- `terminal_pty.rs` — PTY spawn and env
