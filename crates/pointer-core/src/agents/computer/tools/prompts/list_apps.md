### list_apps

List the apps on this computer (Codex Computer Use compatible).

**Default (omit `include_all`):** **currently running** apps plus apps **used in
the last 14 days** (with usage frequency where the OS exposes it). On
**Windows**, the Start Menu launch catalog (`.lnk` / `.exe`) is included.

**You** pick the target from the returned lines — there is no server-side name
filter. Each line is Codex-style text, for example:

- macOS: `WeChat — com.tencent.xinWeChat [frontmost, running, last-used=2026-06-01, uses=12]`
- Windows: `WeChat -- WeChat.exe [running, pid=1234, window=Chat, last-used=2026-06-01, uses=12]`

Pass the app **name** or **bundle / executable identifier** from a line into
**`launch_app`**.

## `include_all` (optional — use sparingly)

**Hard rule:** on the **first** `list_apps` call for a goal, **omit
`include_all`** (or leave it `false`). Set **`include_all`: true** only on a
**second** call when **both** hold:

1. You already called `list_apps` **without** `include_all` for this goal, **and**
2. The target identifier is **still not** in the returned lines.

That retry returns the **full installed catalog** on macOS and Linux; on Windows
the Start Menu is already in the default list, so the retry rarely helps.

**Use `list_apps`** when the identifier came from the user's own words, or to
check whether an app is already running. **Do not** call it again when you
already have an exact identifier from a previous `list_apps` line — pass that
straight to **`launch_app`**.

**macOS:** requires Accessibility permission (same as desktop control).

**`action` field:** describe why you are listing apps, not overlay indices.
