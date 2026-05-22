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

- `src-tauri/tauri.conf.json`: `decorations: false`, macOS `titleBarStyle: Overlay` (native traffic lights).
- Unified top bar in `AppShell.vue`: drag region + brand; Windows/Linux use `WindowControls.vue` (minimize / maximize / close).
- Web (`dev:web`) has no custom title bar — browser chrome unchanged.

## Cross-entry

Same CSS for Tauri and web (`dev:web`). Window chrome is gated with `isTauriRuntime()`; no color branches by platform.
