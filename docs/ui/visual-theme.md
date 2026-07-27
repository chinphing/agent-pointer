# Visual theme (flat, light / dark)

## Tokens

CSS variables in `src/styles/globals.css`:

- `--background`, `--foreground`, `--card`, `--card-elevated`, `--border`
- `--accent`, `--accent-muted`, `--hover`, `--composer-bg`, `--code-bg`
- Semantic: `--success`, `--danger`, `--warning`, `--info`

`html.dark` and default (`:root`) define light; dark overrides on `html.dark`.

### Accent

Brand accent is intentionally **low-saturation** so it reads as a tool, not a
consumer-purple product, especially in light mode:

- Light: `--accent: 250 42% 52%`, `--accent-muted: 250 28% 95%`
- Dark:  `--accent: 250 45% 68%`, `--accent-muted: 250 28% 16%`

Keep saturation ≤ ~50% in light; raise lightness in dark so `text-accent` stays
readable on `--card`. Do not bump saturation back to 80%+ — that reintroduces
the neon-purple feel in light chat.

## UI classes

- `.panel` — flat card (`bg-card` + `border-border`)
- `.panel-elevated` — slightly raised surface
- `.brand-text` — accent-colored title text
- Settings dialog: use semantic tokens (`text-foreground`, `text-muted`, `border-border`, `bg-card`, `bg-hover`, `text-accent`) — **not** hardcoded `slate-*` / `bg-black/*` / `border-white/*`
- Interactive chat controls (e.g. `ask_user`): selected state should derive from `foreground` / `background` (opacity OK) so light and dark both stay readable — avoid fixed gray hex and accent purple fills in light chat
- Sub-agent frame (`SubAgentFrame`): default `border-border` + `bg-card`; failed → `border-danger/35` + `bg-danger/5`; chevrons `text-muted` — not accent purple
- `.settings-input`, `.settings-toggle-track`, `.settings-segment*` — shared controls in settings forms

Do **not** reintroduce `.glass`, `.neon-ring`, aurora body gradients, or heavy `backdrop-blur` in chat UI.

## Blocking overlays (e.g. platform login)

- Scrim: `hsl(var(--foreground) / 0.32)` + light blur — adapts to light/dark (avoid `bg-black/*` / `bg-white` cards).
- Panel: `bg-card`, `border-border`, accent glow optional; controls use semantic tokens (`text-foreground`, `text-muted`, `bg-accent`, `text-danger`).
- Reference: `src/components/auth/PlatformLoginModal.vue`

## Chat assistant status tones

| Status | UI | Notes |
|--------|-----|--------|
| Real failure (`error`) | Red alert card / `text-danger` | e.g. network, balance exhausted |
| User stop (`cancelled`) | Inline muted caption（`text-[11px] text-muted`），**不要**整宽描边横幅 | Always keep the assistant row after stop so「已停止生成」stays visible (empty → compact line; with tools/body → content + caption) |
| Auto-compression summary | Same muted caption family；默认一行「自动压缩摘要 >」，点击展开正文 | `CompressionSummaryBubble` — no card / avatar / border |
| Injected notice | Same muted family as cancel | `【桌面】` / `【提示】` / `【压缩】` |

Do not style user-initiated stop like a system exception.

## Theme preference

- Persisted in **`UserSettings.theme`** (`user_settings.json`) — `light` | `dark` | `system`
- Merged into the effective `ModelSettings.theme` for UI; do not treat session/platform saves as the source of truth for theme
- Settings dialog: cycle system → light → dark; **save theme via `saveUser` before** other `save*` calls (those reload from disk via `applyEffectiveView`)
- Applied via `src/lib/theme.ts` on load, when cycling, and when user settings save
- See also [user-platform-config-split.md](../internals/user-platform-config-split.md)

## Desktop window chrome (Tauri only)

> macOS 红绿灯对齐、reapply/repair 机制、紧凑模式恢复顺序见 [**contributing/macos-window-chrome.md**](../contributing/macos-window-chrome.md)（改窗口 chrome 前必读）。

- Base `tauri.conf.json`: `decorations: false` (Windows/Linux custom chrome).
- macOS `tauri.macos.conf.json`: `decorations: true`, `titleBarStyle: Overlay`, `hiddenTitle: true` — **required** for native traffic lights; `decorations: false` hides them entirely.
- `AppShell.vue` (Manus-style): **sidebar top** = macOS traffic-light inset + drag + collapse only; **main top** = drag strip + Windows/Linux `WindowControls` (top-right). Skills / settings in sidebar footer. Sidebar `260px` ↔ collapsed (`useSidebarCollapse`). No in-app theme toggle (theme remains in Settings).
- macOS: `tauri.macos.conf.json` + `configure_macos_window_chrome()` in `lib.rs` force `decorations: true` and `TitleBarStyle::Overlay`.
- OS detection: `src/lib/desktopOs.ts` (`userAgent` first, then `platform`) — used by `useWindowChrome.ts`.
- Drag: `data-tauri-drag-region` + `-webkit-app-region` / `app-region` in `globals.css`; `startDragging()` fallback on `mousedown` for edge cases.
- Double-click empty title bar toggles maximize (macOS / Windows / Linux).

| Platform | Window controls | Sidebar top inset | Main top |
|----------|-----------------|-------------------|----------|
| macOS | System traffic lights (overlay) | `4.75rem` for lights | Title + drag |
| Windows | Custom `WindowControls` on main top-right | `pl-2` | Drag + min/max/close |
| Linux | Custom `WindowControls` on main top-right (same as Windows) | `pl-2` | Drag + min/max/close |
| Web (`web:dev`) | Browser chrome | Brand in sidebar top when expanded | Title only |

- Web (`web:dev`) has no custom title bar — browser chrome unchanged.

## Cross-entry

Same CSS for Tauri and web (`web:dev`). Window chrome is gated with `isTauriRuntime()`; no color branches by platform.
