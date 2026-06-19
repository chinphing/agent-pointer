### list_apps

List the apps on this computer (Codex Computer Use compatible).

**Default (omit `include_all`):** **currently running** apps plus apps **used in the last 14 days**
(with usage frequency when available on macOS Spotlight and Windows UserAssist).

**You** pick the target from the returned lines — there is no server-side name filter.

Requires **`goal`** and optional **`action`** when desktop instructions require it.

Each line uses Codex-style text, for example:

- macOS: `WeChat — com.tencent.xinWeChat [frontmost, running, last-used=2026-06-01, uses=12]`
- Windows/Linux: `WeChat -- WeChat.exe [running, pid=1234, window=Chat, last-used=2026-06-01, uses=12]`
- Windows/Linux (tray-only, still running): `WeChat -- WeChat.exe [running, pid=1234, last-used=2026-06-01, uses=12]`
- Installed but not running (only after **`include_all`: true** retry): `Calculator -- Calculator.app` (macOS)

Pass the app **name** or **bundle / executable identifier** from a line into **`launch_app`**.

## `include_all` (optional — use sparingly)

**Hard rule:** On the **first** `list_apps` call for a goal, **omit `include_all`**
(or leave it `false`). Do **not** set **`include_all`: true** preemptively.

Set **`include_all`: true** only on a **second** `list_apps` call when **both** are true:

1. You already called `list_apps` **without** `include_all` for this goal, **and**
2. The target app identifier is **still not** in the returned lines.

That retry returns the **full installed catalog** (Applications / Start Menu / `.desktop`).

**When to use `list_apps`**
- Before **`launch_app`** when the exact app identifier is uncertain
- Check whether an app is already running

**When not to use**
- Do **not** call **`list_apps`** when the user already named the app — call **`launch_app`** directly
- Do **not** set **`include_all`: true** on the first list for a goal

**macOS:** Requires Accessibility permission (same as desktop control).

**`action` field:** Describe why you are listing apps, not overlay indices.
