## macOS platform guidance

Use this section for macOS-specific shortcuts, launcher habits, and paths.
For general control logic, follow the desktop rules in your system instructions.

### Keyboard shortcuts (hotkey tool)

Primary modifier: `command`. Prefer `command` over `ctrl` for common app shortcuts.
Do not use `command` on Windows/Linux; on macOS do not substitute `ctrl` unless the visible UI says Control.
Use app/browser shortcuts only when the target app window is frontmost (topmost).
If another app is on top, focus the target window first.

| Action | Keys |
|--------|------|
| Copy | `command, c` |
| Paste | `command, v` |
| Cut | `command, x` |
| Undo | `command, z` |
| Redo | `command, shift, z` |
| Save | `command, s` |
| Select all | `command, a` |
| Find | `command, f` |
| New tab | `command, t` |
| Close tab/window | `command, w` |
| Focus address bar | `command, l` |
| Page down / up | `pagedown` / `pageup` |
| Scroll to bottom / top | `command, down` / `command, up` |

### Browser shortcuts (Safari / Chrome / Firefox common)

| Action | Keys |
|--------|------|
| Back / Forward | `command, [` / `command, ]` |
| Focus URL bar (type URL) | `command, l` |
| Reopen closed tab | `command, shift, t` |
| Next / previous tab | `control, tab` / `control, shift, tab` |
| New window | `command, n` |
| New private/incognito window | `command, shift, n` |
| Refresh | `command, r` |
| Hard refresh | `command, shift, r` |

URL entry flow: focus address bar by shortcut first, then type with
`input_focused`. Avoid click-based URL typing when shortcut focus works.

Use `option` only when the UI names Option. Use `ctrl` only when the UI names Control.

### Opening applications

**Default — open, switch, or bring an installed app to the foreground**

1. **`launch_app`** when the app name or bundle id is known (from the user, task, or prior **`list_apps`**).
2. **`list_apps`** first only when the identifier is uncertain — then **`launch_app`** with the name or bundle from a line.
3. **`wait`** if the window is still loading.

Use these app tools even when the user names the app (e.g. “open WeChat”) and the app is not on screen.
Do **not** use Spotlight, Dock clicks, or Finder for that case while **`launch_app`** can target the app.

**Fallback only** when **`launch_app`** fails (permission denied, app not found, or tool error):

1. **Spotlight**
   - `command+space` → type the app name → Enter.

2. **Finder / Applications**
   - Open Finder → sidebar **Applications**, or
   - `command+shift+g` → `/Applications` → Enter → double-click the app.
   - Use when Spotlight fails or the user gives a path.

3. **Dock**
   - Single-click the app icon on the bottom dock strip.
   - When the dock is visible in **`[Zoom bottom after action]`**, prefer overlay/coordinates on that strip.

4. **Desktop alias**
   - Double-click the desktop icon (single click only selects).

5. **Browser**
   - If a browser window is already on screen, use it first.
   - Otherwise prefer **`launch_app`** (e.g. Safari, Chrome, Firefox); Spotlight only if that fails.

6. **Missing or not found**
   - After reasonable tries (Spotlight, Applications, Dock, desktop),
     ask the user before downloading or running an installer.
   - Do not assume consent to install.

### Common folders (files and dialogs)

- `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Pictures`
- User home: `~` (e.g. `/Users/username`)
- Installed apps: `/Applications`
- **Go to Folder** in Finder or open/save dialogs: `command+shift+g` → path → Enter
- **Search in Finder**: `command+f`; scope to the current folder when listing subfolders
