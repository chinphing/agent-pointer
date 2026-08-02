# External links (default browser)

In-app `http` / `https` links open in the **OS default browser** (desktop) or a **new tab** (web). They must not navigate the main Tauri webview away from the SPA.

## Layers

1. **Frontend capture** — `installExternalLinkClickHandler` in `main.ts` intercepts same-document left-clicks on external anchors and calls `openExternalUrl` (`@tauri-apps/plugin-shell` / `window.open`).
2. **Markdown / message bodies** — `useMarkdownExternalLinks` also routes through `openExternalUrl` (belt-and-suspenders with the global handler).
3. **Tauri native** — main window `on_navigation` allows only localhost / `tauri.localhost` (and non-http schemes); other `http(s)` URLs are opened via `open_url_in_browser` and cancelled. `on_new_window` keeps WeCom (`work.weixin.qq.com`) QR popups in-app; other `http(s)` popups open in the system browser.

## UI convention

Prefer `@click` + `openExternalUrl` (or a plain `href` that the global handler can see). Avoid relying on `target="_blank"` alone in the desktop app — Tauri denies unknown new windows unless handled above.
