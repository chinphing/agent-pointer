# Computer agent — app list/launch (Codex aligned)

Cross-platform application discovery and launch aligned with [OpenAI Codex Computer Use](https://developers.openai.com/codex/app/computer-use).

Reference implementation: [open-codex-computer-use](https://github.com/iFurySt/open-codex-computer-use) (open-source Codex Computer Use mirror).

## Tools

| Tool | Codex equivalent | Purpose |
|------|------------------|---------|
| `list_apps` | `list_apps` | List installed apps (+ running/recent enrichment); model picks target |
| `launch_app` | `launch_app` | Launch or activate by `app` name / bundle id / exe |

Implementation: `crates/pointer-core/src/platform/app_access/`.

## list_apps behavior

- **`include_all`** (optional, default **`false`** — omit on first call):
  - **First call:** omit `include_all` → running apps plus apps used in the last 14 days. Use for typical launch/switch tasks.
  - **Retry only:** set `include_all: true` when the first list did not contain the target app → full installed catalog.
  - Do **not** set `include_all: true` preemptively on the first list for a goal.
- Returns plain-text lines for the model (Codex format); no server-side name filter.
- **macOS (`include_all: true`):** Catalog from `/Applications`, `/System/Applications`, `~/Applications`, and `/System/Library/CoreServices` via Spotlight `mdfind` plus filesystem `.app` scan. Metadata comes from `mdls` when available, otherwise `Info.plist` (some apps such as BaiduNetdisk are missing from Spotlight metadata). Background agents (`LSBackgroundOnly`, `LSUIElement`) are excluded.
- **macOS (`include_all: false`):** Running user-facing apps plus Spotlight entries with `last-used` within 14 days.
- **Windows (default and `include_all: true`):** Start Menu catalog (recursive `.lnk` and `.exe` under ProgramData and user Start Menu Programs), merged with visible running windows, tray-only processes, and UserAssist recent usage (14 days). `include_all` is a no-op on Windows; use it only for macOS/Linux full-catalog retries.
- **Linux (`include_all: true`):** Full `.desktop` catalog from standard application directories, merged with `wmctrl` windows, tray-only `/proc` processes, and `recently-used.xbel` (14 days).
- **Linux (`include_all: false`):** Running windows + tray-only processes plus XBEL entries within 14 days.

Tray-only running apps (e.g. WeChat minimized to tray) appear in the list with `running` and `pid` but no `window` title.

## launch_app behavior

- Parameter **`app`**: display name, bundle identifier (macOS), executable (Windows), or `.desktop` id / display name (Linux).
- **Default:** activate running instance; otherwise launch.
- **`new_instance: true`:** skip activation, start new instance (`open -n` on macOS).
- **`activate_only: true`:** focus only; do not launch if not running.
- **Windows tray-only:** tries visible window → hidden top-level window → relaunch same `.exe` to restore UI; launch fallback uses the running process image path when Start Menu lookup fails.
- **Linux tray-only (X11):** tries `wmctrl` window focus → `gtk-launch` via matching `.desktop` entry → relaunch `/proc/PID/exe`. Verification treats a running pid without a wmctrl window as `running: true, frontmost: false`.
- **macOS tray-only:** tries `NSWorkspace` activate → `open -a` restore when activation does not frontmost. Host verification requires the app to be **frontmost with an on-screen window** (layer 0, ≥50×50 px via `CGWindowList`); `NSWorkspace.frontmostApplication` alone is not enough — tray/hidden apps report `running: true, frontmost: false`.
- **Launch verification (all platforms):** success requires `running && frontmost` after polling (4s launch / 2s activate). A pre-existing tray-only process no longer passes with `Verified:` in ~18ms.

Example macOS line:

```text
WeChat — com.tencent.xinWeChat [frontmost, running, last-used=2026-06-01, uses=12]
```

Example Linux line (installed, not running):

```text
Firefox -- firefox
```

Example Linux line (recent, no uses):

```text
Firefox -- firefox [last-used=2026-06-10]
```

## Agent usage

1. `list_apps` when the app identifier is uncertain — **omit `include_all` on the first call**.
2. Model selects a line from the text result; if the target is missing, **retry `list_apps` once with `include_all: true`**.
3. `launch_app` with `app` set to the name or bundle / exe id.
4. `wait` if the window is still loading.

## Pipeline

- Operation family: `AppAccess`
- Skips positioning (no coordinates)
- Host verify only (OS API + tool text parsing); **no Verify LLM**

## Permissions

- **macOS:** Accessibility (list + launch/activate)
- **Windows:** Logged-in desktop session
- **Linux (X11):** Logged-in desktop session; `wmctrl` and `gtk-launch` for list/activate/launch. **Wayland:** `wmctrl` is limited or unavailable; list/activate may miss windows or fail — full DE-specific activation is not implemented.

## Capture monitor (`computerAutoSwitchMonitor`)

User setting in **设置 → 智能体 → 电脑操控选项**:

| 值 | 行为 |
|----|------|
| **开启（默认）** | 多屏时不弹选择框，默认主屏；`launch_app` 或鼠标/键盘等桌面操作成功后，若前台/目标应用窗口在另一块屏，自动切换截屏目标（macOS：`CGWindowList` + `CGDisplayBounds` → `xcap:{displayId}`；Windows：`GetForegroundWindow` / `GetWindowRect` → `monitor_id_at_global_point`；Linux X11：`wmctrl` 活跃窗口 → `monitor_id_at_global_point`） |
| **关闭** | 多屏时按系统「显示器排列」方位手动点选屏幕；不随应用窗口自动切换 |
