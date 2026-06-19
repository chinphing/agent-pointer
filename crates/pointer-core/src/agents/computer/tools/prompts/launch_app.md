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
1. **`launch_app`** directly when the app name or bundle / exe id is known
2. **`list_apps`** only when the identifier is uncertain — then **`launch_app`** with the chosen line
3. **`wait`** if the window is still loading

**Hard rule — before UI fallbacks**

When the goal is to open, switch, or foreground an **installed** app by name, call **`launch_app`**
(this tool) first. Do **not** use Spotlight, Start search, application menu, Dock, or taskbar
clicks for that goal unless **`launch_app`** already failed or returned permission denied.

**macOS:** Requires Accessibility permission. New instances use `open -n`.
**Windows:** Use the executable name reported by **`list_apps`** (e.g. `notepad.exe`).

**`action` field:** Describe the app or intent in words, not overlay indices.
