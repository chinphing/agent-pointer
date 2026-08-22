# Visual theme (flat, light / dark)

## Tokens

CSS variables in `src/styles/globals.css`:

- `--background`, `--foreground`, `--card`, `--card-elevated`, `--border`
- `--accent`, `--accent-muted`, `--accent-foreground`, `--hover`, `--composer-bg`, `--code-bg`, `--fence-bg`
  (`--composer-bg` fills `.composer-shell`; light = white, not `--card-elevated`)
- Semantic: `--success`, `--danger`, `--warning`, `--info`
- Search hit: `--search-mark`

`html.dark` and default (`:root`) define light; dark overrides on `html.dark`.

Surfaces are **neutral gray** (Codex / Apple grouped). Hue stays near 240 with
near-zero saturation. Dark canvas is charcoal, not blue-black.

**Shell:** `--shell-chat` / `--shell-sidebar` (classes `.shell-chat` /
`.shell-sidebar`). **Left rail** uses `.shell-sidebar`. **Conversation
column and the right workspace panel** both use `.shell-chat` so light
and dark keep the same contrast: light = white chat + workspace, gray
left rail; dark = both panes darker than the left rail. Do not paint
the workspace with `bg-card` — in dark mode `--card` matches the left
rail and the right pane looks brighter than chat. The workspace
terminal pane (xterm `theme.background` and `.xterm-viewport`) uses
`--shell-chat` for the same reason.

Sticky overlays on `.shell-chat` (task board, workspace find) stay
opaque `.shell-chat`, not `bg-background/95` or `backdrop-blur`.

New UI must use these tokens (or Tailwind aliases `bg-background`, `text-muted`,
`border-border`, `bg-accent`, …). Do **not** add `slate-*` / `zinc-*` / hardcoded
hex / `bg-black/*` scrims in product UI. Diff views (`DiffView` and related) are
exempt and keep their own colors.

### Accent

One calm **system blue** for primary actions, toggles-on, focus rings, and links.
Do not tint selected list rows or avatars with accent.

- Light: `--accent: 211 100% 46%`, `--accent-muted: 211 80% 96%`, `--accent-foreground: 0 0% 100%`
- Dark:  `--accent: 211 100% 58%`, `--accent-muted: 211 40% 16%`, `--accent-foreground: 0 0% 100%`

Primary buttons: `bg-accent text-accent-foreground` (never hardcode `text-white`
on accent fills). Selected rows: `bg-hover` / `bg-foreground/10`, not
`bg-accent-muted text-accent`.

Composer toolbar icons (clip, agent, mode), the chat top-bar project
folder, and the workspace panel header folder use `text-muted`, not
accent or `--warning`. Send / stop stay semantic (`bg-accent` / `text-danger`).

## Chat column width

Conversation content (messages, composer, change summary) uses a centered
`.chat-column` inside `.chat-shell`. Width follows the **middle pane**, not the
viewport, so it stays in sync when the left sidebar collapses or the right
workspace panel is resized:

- `AppShell` middle pane is `.chat-main` with `container-type: inline-size`
- `.chat-column` max width: `min(1024px, 100%)` — grows with the padded middle
  pane, capped at 1024px
- `.chat-shell` horizontal padding (side gutters): `px-8` by default; `6rem`
  when the chat container is ≥720px (container query, not `md:` viewport)
- `.chat-scroll-area` uses `overflow-x: hidden` so a narrow middle pane cannot
  grow a full-pane horizontal scrollbar (avatar overhang / long tool lines).
  Hide `.message-avatar-slot` under `@container chat (max-width: 719px)` for the
  same reason — see [mobile-chat.md](mobile-chat.md).
- Chat transcript / welcome / sidebar lists use `.auto-hide-scrollbar` (global):
  thumb hidden at rest, visible while scrolling via `showScrollbarWhileScrolling`
  (`src/lib/autoHideScrollbar.ts`).

Do not switch this back to viewport-only `max-w-3xl` / `md:px-*` — that drifts
from available width while the workspace panel is open.

## UI classes

- `.panel` — flat card (`bg-card` + `border-border`)
- `.panel-elevated` — slightly raised surface
- `.brand-text` — title text (`text-foreground`)
- Markdown GFM / HTML tables (`.md-body .table-wrapper`): rounded outer border; `th`/`td` theme cells (works without `<thead>`/`<tbody>`); honor GFM align + HTML column `width` / status colors; see [markdown-media-boundaries.md](markdown-media-boundaries.md), `markdownConfig.ts` / `globals.css`
- Markdown fenced code (`.md-body .code-block`): same palette as tables — body `--card` (token `--fence-bg` aliases it), language row `.fence-block-lang` uses `--hover` like `th`. Copy sits in that header (flex `items-center`), not `absolute` on the whole block; see `markdownConfig.ts` / `useMarkdownCodeCopy`
- Markdown inline code (`.md-body code`): accent text only, **no** `--code-bg` chip / padding
- Markdown charts (`.md-body .md-chart`): same card chrome; Chart.js from `chartjs`/`chart` JSON fences; theme axis/legend colors from CSS variables; see [markdown-charts.md](markdown-charts.md)
- Markdown SVG diagrams (`.md-body .md-svg`): same card chrome; sanitized `svg` fences; see [markdown-svg.md](markdown-svg.md)
- Settings dialog: use semantic tokens (`text-foreground`, `text-muted`, `border-border`, `bg-card`, `bg-hover`, `text-accent`) — **not** hardcoded `slate-*` / `bg-black/*` / `border-white/*`
- `.fence-block` — table chrome (`rounded-lg border-border`, body `--card`); `.fence-block-header` matches table `th` (`--hover` fill, bottom border, semibold)
- Interactive chat controls (e.g. `ask_user`): **the question** is `.fence-block-header`; options sit in the fence body. Tool line stays「询问用户」only.
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
- Preference `system` listens to `prefers-color-scheme` (and Tauri `onThemeChanged`) so the UI switches when OS appearance changes; locked `light` / `dark` do not. Desktop also calls `setTheme(null)` so native chrome follows the OS.
- **Cross-platform / cross-entry:** the visible UI is always `html.light` / `html.dark` + CSS tokens (same for Tauri and `web:dev`). Web never calls window theme APIs. Desktop adds `setTheme` / `onThemeChanged` as a WebView backup.

| Surface | Follow-system live update | Notes |
|---------|---------------------------|--------|
| Web | `matchMedia` | Browser chrome stays with the browser |
| macOS app | `matchMedia` + `onThemeChanged` | Native traffic lights / overlay chrome follow `setTheme`; `setTheme` is **app-wide** |
| Windows app | `matchMedia` + `onThemeChanged` | Custom title buttons; theme is CSS. WebView2 usually tracks Windows app mode |
| Linux app | same APIs | Weakest live path: some DE/portal/WebKitGTK builds only refresh `prefers-color-scheme` after focus or restart; `onThemeChanged` is the fallback. `setTheme` is **app-wide** |

Canvas charts sample token colors at mount; xterm watches `html` class. Neither is OS-specific.
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
| macOS | System traffic lights (overlay) | `4.75rem` for lights; **dropped in native fullscreen** | Title + drag |
| Windows | Custom `WindowControls` on main top-right | `pl-2` | Drag + min/max/close |
| Linux | Custom `WindowControls` on main top-right (same as Windows) | `pl-2` | Drag + min/max/close |
| Web (`web:dev`) | Browser chrome | Brand in sidebar top when expanded | Title only |

- Web (`web:dev`) has no custom title bar — browser chrome unchanged.

## Cross-entry

Same CSS for Tauri and web (`web:dev`). Window chrome is gated with `isTauriRuntime()`; no color branches by platform.
