### launch_app

Launch or bring an application to the foreground (Codex Computer Use compatible).

Requires **`goal`**, **`app`** (display name, bundle identifier, or executable such as `notepad.exe`),
and optional **`action`** when desktop instructions require it.

Optional:
- **`new_instance`**: `true` only when the user **explicitly** wants another window/process
- **`activate_only`**: `true` to focus a running instance **without** launching if none is running

**Default behavior**

1. If the app is already running → **bring it to the foreground**
2. If not running → **launch** it

**Host verification**

After launch or activate, the runtime **polls the OS** (up to ~4s) to confirm the app is
running. Activate also requires the app to become **frontmost**. Failed verification returns
**FAILED** even when the launch command exited successfully.

**Flow**
1. **`list_apps`** first when the identifier came from the user's own words
2. **`launch_app`** with a name or bundle / exe id copied verbatim from a `list_apps` line
3. **`wait`** if the window is still loading

**Bundle names are not what the user says**

Installed names are often longer, localized, or vendor-prefixed than the spoken
name. `launch_app` resolves by exact match, so a spoken name usually misses and
costs a launch attempt plus the ~4 s verification poll.

On every platform, when the name came from the user rather than a previous
`list_apps` line, call `list_apps` first and copy an identifier from it. Skip
that only when you already hold an exact identifier.

If `launch_app` still fails, its error lists similar installed names — pass one
verbatim instead of guessing again.

**Hard rule — before UI fallbacks**

When the goal is to open, switch, or foreground an **installed** app by name, call **`launch_app`**
(this tool) first. Do **not** use Spotlight, Start search, application menu, Dock, or taskbar
clicks for that goal unless **`launch_app`** already failed or returned permission denied.

**macOS:** Requires Accessibility permission. New instances use `open -n`.
**Windows:** Use the executable name reported by **`list_apps`** (e.g. `notepad.exe`).

**`action` field:** Describe the app or intent in words, not overlay indices.
