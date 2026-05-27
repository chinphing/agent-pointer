## Windows platform guidance

Use this section for Windows-specific shortcuts, launcher habits, and paths.
For general control logic, follow the desktop rules in your system instructions.

### Keyboard shortcuts (hotkey tool)

Primary modifier: `ctrl`. Do not use `command` on Windows.
Use app/browser shortcuts only when the target app window is frontmost (topmost).
If another app is on top, focus the target window first.

| Action | Keys |
|--------|------|
| Copy | `ctrl, c` |
| Paste | `ctrl, v` |
| Cut | `ctrl, x` |
| Undo | `ctrl, z` |
| Redo | `ctrl, y` |
| Save | `ctrl, s` |
| Select all | `ctrl, a` |
| Find | `ctrl, f` |
| New tab | `ctrl, t` |
| Close tab | `ctrl, w` |
| Close window | `alt, f4` |
| Focus address bar | `ctrl, l` |
| Refresh | `ctrl, r` or `f5` |
| Page down / up | `pagedown` / `pageup` |
| Scroll to bottom / top | `ctrl, end` / `ctrl, home` |

### Browser shortcuts (Chrome / Edge / Firefox common)

| Action | Keys |
|--------|------|
| Back / Forward | `alt, left` / `alt, right` |
| Focus URL bar (type URL) | `ctrl, l` or `alt, d` |
| Reopen closed tab | `ctrl, shift, t` |
| Next / previous tab | `ctrl, tab` / `ctrl, shift, tab` |
| New window | `ctrl, n` |
| New private/incognito window | `ctrl, shift, n` |
| Hard refresh | `ctrl, shift, r` or `ctrl, f5` |

URL entry flow: focus address bar by shortcut first, then type with
`type_text_at_focused`. Avoid click-based URL typing when shortcut focus works.

Use `alt` for menu accelerators or explicit Alt shortcuts only.
Use `win` / `meta` when the step clearly needs the Windows key (Start, search, snap).

### Opening applications

Prefer the fastest visible path; use **hotkey** for launcher shortcuts when reliable.

1. **Search (default for “open app X” by name)**
   - `win+s` → type the app or file name → Enter.
   - Primary way to launch installed software by name.

2. **Start menu**
   - Press `win` → type the app name → Enter.
   - Use when search is unavailable or the Start menu is already open.

3. **Run dialog (advanced)**
   - `win+r` → type executable or known command → Enter.
   - Use only when the user or visible UI names a Run command.

4. **Desktop shortcut**
   - **Double-click** the desktop icon.
   - Single click only selects; use **`mouse:double_click_at`** at **Location** **(x,y)**.

5. **Taskbar**
   - Single-click a pinned icon on the bottom taskbar strip.
   - When the taskbar is visible in **`[Zoom bottom after action]`**, prefer overlay/coordinates on that strip.

6. **File Explorer (open a folder, not an app)**
   - `win+e`, or `win+r` with a path / `explorer <path>`.
   - Not a substitute for launching apps by name.

7. **Terminal (PowerShell only — not general app launch)**
   - `win` → type `powershell` → Enter, or `win+r` → `powershell` → Enter.
   - Use **Windows PowerShell** for terminal/code tasks; never `cmd.exe` unless the user requires it.

8. **Small window after launch**
   - Maximize the window, use `win+up` on the focused window, or `f11` when the app supports full screen (e.g. browsers).

9. **Missing or not found**
   - After reasonable tries (search, Start menu, desktop, taskbar, common install paths),
     ask the user before downloading or running an installer.
   - Do not assume consent to install.

### Common folders (files and dialogs)

- Native paths use `\` (in JSON escape as `\\`).
- `%USERPROFILE%\Desktop`, `%USERPROFILE%\Downloads`, `%USERPROFILE%\Documents`
- In File Explorer or open/upload dialogs: focus the path bar, paste the folder path, Enter
- Search within a folder: `ctrl+f` in Explorer or the dialog
