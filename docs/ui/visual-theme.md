# Visual theme (flat, light / dark)

## Tokens

CSS variables in `src/styles/globals.css`:

- `--background`, `--foreground`, `--card`, `--card-elevated`, `--border`
- `--accent`, `--accent-muted`, `--hover`, `--composer-bg`, `--code-bg`
- Semantic: `--success`, `--danger`, `--warning`, `--info`

`html.dark` and default (`:root`) define light; dark overrides on `html.dark`.

## UI classes

- `.panel` — flat card (`bg-card` + `border-border`)
- `.panel-elevated` — slightly raised surface
- `.brand-text` — accent-colored title text
- Settings dialog: use semantic tokens (`text-foreground`, `text-muted`, `border-border`, `bg-card`, `bg-hover`, `text-accent`) — **not** hardcoded `slate-*` / `bg-black/*` / `border-white/*`
- `.settings-input`, `.settings-toggle-track`, `.settings-segment*` — shared controls in settings forms

Do **not** reintroduce `.glass`, `.neon-ring`, aurora body gradients, or heavy `backdrop-blur` in chat UI.

## Blocking overlays (e.g. platform login)

- Scrim: `hsl(var(--foreground) / 0.32)` + light blur — adapts to light/dark (avoid `bg-black/*` / `bg-white` cards).
- Panel: `bg-card`, `border-border`, accent glow optional; controls use semantic tokens (`text-foreground`, `text-muted`, `bg-accent`, `text-danger`).
- Reference: `src/components/auth/PlatformLoginModal.vue`

## Theme preference

- Setting: `ModelSettings.theme` — `light` | `dark` | `system`
- Sidebar cycles: system → light → dark
- Applied via `src/lib/theme.ts` on load and when settings save

## Desktop window chrome (Tauri only)

- Base `tauri.conf.json`: `decorations: false` (Windows/Linux custom chrome).
- macOS `tauri.macos.conf.json`: `decorations: true`, `titleBarStyle: Overlay`, `hiddenTitle: true` — **required** for native traffic lights; `decorations: false` hides them entirely.
- `AppShell.vue` (Manus-style): **sidebar top** (transparent) = macOS traffic-light inset + drag + collapse; **main top** = drag strip + Windows `WindowControls` (top-right). Linux keeps `WindowControls` in sidebar top. Skills / settings in sidebar footer. Sidebar `260px` ↔ collapsed (`useSidebarCollapse`). No in-app theme toggle (theme remains in Settings).
- macOS: `tauri.macos.conf.json` + `configure_macos_window_chrome()` in `lib.rs` force `decorations: true` and `TitleBarStyle::Overlay`.
- OS detection: `src/lib/desktopOs.ts` (`userAgent` first, then `platform`) — used by `useWindowChrome.ts`.
- Drag: `data-tauri-drag-region` + `-webkit-app-region` / `app-region` in `globals.css`; `startDragging()` fallback on `mousedown` for edge cases.
- Double-click empty title bar toggles maximize (macOS / Windows / Linux).

| Platform | Window controls | Sidebar top inset | Main top |
|----------|-----------------|-------------------|----------|
| macOS | System traffic lights (overlay) | `4.75rem` for lights | Title + drag |
| Windows | Custom `WindowControls` on main top-right | `pl-2` | Drag + min/max/close |
| Linux | Custom `WindowControls` in sidebar top | `pl-2` | Drag only |
| Web (`dev:web`) | Browser chrome | Brand in sidebar top when expanded | Title only |

- Web (`dev:web`) has no custom title bar — browser chrome unchanged.

## Cross-entry

Same CSS for Tauri and web (`dev:web`). Window chrome is gated with `isTauriRuntime()`; no color branches by platform.
