## Linux platform guidance

Use this section for Linux-specific shortcuts, launcher habits, and paths.
Desktop environments differ (GNOME, KDE, XFCE, etc.); prefer what is visible on screen.
For general control logic, follow the desktop rules in your system instructions.

### Keyboard shortcuts (hotkey tool)

Primary modifier: `ctrl`. Do not use `command` unless the visible UI explicitly names it.
Use app/browser shortcuts only when the target app window is frontmost (topmost).
If another app is on top, focus the target window first.

| Action | Keys |
|--------|------|
| Copy | `ctrl, c` |
| Paste | `ctrl, v` |
| Cut | `ctrl, x` |
| Undo | `ctrl, z` |
| Redo | `ctrl, shift, z` |
| Save | `ctrl, s` |
| Select all | `ctrl, a` |
| Find | `ctrl, f` |
| New tab | `ctrl, t` |
| Close tab/window | `ctrl, w` or `alt, f4` |
| Focus address bar / location | `ctrl, l` |
| Page down / up | `pagedown` / `pageup` |
| Scroll to bottom / top | `ctrl, end` / `ctrl, home` |

### Browser shortcuts (Chrome / Chromium / Firefox common)

| Action | Keys |
|--------|------|
| Back / Forward | `alt, left` / `alt, right` |
| Focus URL bar (type URL) | `ctrl, l` or `alt, d` |
| Reopen closed tab | `ctrl, shift, t` |
| Next / previous tab | `ctrl, tab` / `ctrl, shift, tab` |
| New window | `ctrl, n` |
| New private/incognito window | `ctrl, shift, n` |
| Refresh / hard refresh | `ctrl, r` or `f5` / `ctrl, shift, r` or `ctrl, f5` |

URL entry flow: focus address bar by shortcut first, then type with
`type_text_at_focused`. Avoid click-based URL typing when shortcut focus works.

Global shortcuts vary by desktop environment.
For app-local shortcuts, follow the visible in-app hint when present.

### Opening applications

Launcher behavior varies; use the pattern that matches the current desktop UI.

1. **Application search (try first on many desktops)**
   - Press `super` (Windows logo key) → type the app name → Enter.
   - Works on GNOME, KDE, and others with a unified search overlay.

2. **Activities / overview (GNOME)**
   - `super` → search field → type app name → Enter.

3. **Application menu**
   - Click the visible application menu or launcher icon (often bottom-left or top-left)
     when it appears in the screenshot, then choose or search for the app.

4. **Run command (KDE, XFCE, and similar)**
   - `alt+f2` → type command or app name → Enter when the run dialog is supported.

5. **File manager → installed apps**
   - Open the file manager from the menu or `super` search,
     then browse menu-installed applications if a software folder is shown.

6. **Desktop launcher**
   - Double-click a `.desktop` icon on the desktop (single click may only select).

7. **Panel / dock**
   - Single-click a pinned launcher on a bottom or side panel when visible in
     **`[Zoom bottom after action]`** or the main screen.

8. **Terminal (`xdg-open`)**
   - `xdg-open <path>` opens a file or URL with the default app.
   - Use for paths, not as the primary way to launch apps by friendly name.

9. **Missing or not found**
   - After reasonable tries (search, menu, panel, desktop),
     ask the user before downloading or running an installer.
   - Do not assume consent to install.

### Common folders (files and dialogs)

- `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Pictures`
- User home: `~` (e.g. `/home/username`)
- File manager location bar: `ctrl+l` → path → Enter
- Search in folder: `ctrl+f`; enable “search subfolders” / recursive when listing nested files
