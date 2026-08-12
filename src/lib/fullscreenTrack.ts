/**
 * Track a Tauri window fullscreen→windowed transition while an overlay is open.
 *
 * On macOS, pressing Escape while the OS-level window is fullscreen is consumed
 * by the system to exit fullscreen — the DOM keydown never reaches the page,
 * so fullscreen overlays (diagram zoom, image preview) cannot close
 * themselves. When the window leaves fullscreen, invoke `onExit` so the
 * overlay follows the user's intent (Esc = close the topmost layer).
 *
 * This module intentionally does NOT import `./runtime`: that module evaluates
 * `import.meta.env` at load time (vite-only), which would break bundling this
 * pure-DOM helper outside a vite build.
 *
 * Returns an unlisten function (no-op when not in a Tauri runtime or the
 * window is not fullscreen).
 */
import { isTauri } from '@tauri-apps/api/core'

/** Same origin rules as `runtime.ts`'s `isTauriRuntime` (kept in sync). */
function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') return false
  if (!isTauri()) return false
  const { protocol, hostname } = window.location
  if (protocol === 'tauri:') return true
  if (hostname === 'localhost' || hostname === '127.0.0.1') return true
  if (hostname.endsWith('.localhost')) return true
  return false
}

export async function trackFullscreenExit(onExit: () => void): Promise<() => void> {
  if (!isTauriRuntime()) return () => {}
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    if (!(await win.isFullscreen())) return () => {}
    const unlisten = await win.onResized(() => {
      // Query after a tick: the fullscreen state may lag the resize event.
      window.setTimeout(() => {
        win
          .isFullscreen()
          .then(fs => {
            if (!fs) onExit()
          })
          .catch(() => {})
      }, 80)
    })
    return () => {
      void unlisten()
    }
  } catch (err) {
    console.warn('[fullscreenTrack] fullscreen exit tracking failed', err)
    return () => {}
  }
}
