import { isTauri } from '@tauri-apps/api/core'

/** Origins where the bundled app may call Tauri IPC (main window), not remote cloud WebViews. */
function isAppBundledOrigin(): boolean {
  if (typeof window === 'undefined') return false
  const { protocol, hostname } = window.location
  if (protocol === 'tauri:') return true
  if (hostname === 'localhost' || hostname === '127.0.0.1') return true
  if (hostname.endsWith('.localhost')) return true
  return false
}

/**
 * True when this frontend may use Tauri IPC (desktop main window).
 * Cloud agent windows load remote pointer-server URLs; they still expose Tauri internals
 * but IPC is blocked — those pages must use the HTTP API instead.
 */
export function isTauriRuntime(): boolean {
  return isTauri() && isAppBundledOrigin()
}

function resolveWebApiBase(): string {
  const env = import.meta.env.VITE_WEB_API_BASE
  // Explicit empty / unset = same-origin (integrated server UI, or Vite /api proxy).
  // Cross-origin needs VITE_WEB_API_BASE=http://… and POINTER_SERVER_CORS_ORIGINS.
  if (env !== undefined) {
    return String(env)
  }
  return ''
}

export const WEB_API_BASE = resolveWebApiBase()
