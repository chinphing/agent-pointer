## macOS platform guidance

Use this section for macOS-specific shortcuts, launcher habits, and paths.
For general control logic, follow the desktop rules in your system instructions.

### Keyboard shortcuts (hotkey tool)

Primary modifier: `command`. Prefer `command` over `ctrl` for common app shortcuts.
Do not use `command` on Windows/Linux; on macOS do not substitute `ctrl` unless the visible UI says Control.

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

Use `option` only when the UI names Option. Use `ctrl` only when the UI names Control.

### Opening applications

Prefer the fastest visible path; use **hotkey** for launcher shortcuts when reliable.

1. **Spotlight (default for “open app X” by name)**
   - `command+space` → type the app name → Enter.
   - Fastest when the user names an app or file and it is installed.

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
   - Otherwise open via Spotlight (e.g. Safari, Chrome, Firefox).

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
