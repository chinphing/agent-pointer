/** Desktop OS detection for Tauri window chrome (macOS vs Windows vs Linux). */

export type DesktopOs = 'macos' | 'windows' | 'linux' | 'unknown'

type NavLike = Pick<Navigator, 'userAgent' | 'platform'>

/**
 * Detect host OS from navigator (Tauri WebView2 / WKWebView).
 * Uses userAgent first — `navigator.platform` is deprecated and often empty on Windows.
 */
export function detectDesktopOs(nav?: NavLike): DesktopOs {
  const n = nav ?? (typeof navigator !== 'undefined' ? navigator : undefined)
  if (!n) return 'unknown'

  const ua = n.userAgent
  if (/Macintosh|Mac OS X/i.test(ua) && !/iPhone|iPad|iPod|Android/i.test(ua)) {
    return 'macos'
  }
  if (/Windows/i.test(ua)) return 'windows'
  if (/Linux/i.test(ua) && !/Android/i.test(ua)) return 'linux'

  const platform = n.platform
  if (/Mac/i.test(platform) && !/iPhone|iPad|iPod/i.test(platform)) return 'macos'
  if (/Win/i.test(platform)) return 'windows'
  if (/Linux/i.test(platform)) return 'linux'

  return 'unknown'
}

/** macOS overlay: horizontal inset for traffic lights (leading). */
export const MAC_TITLEBAR_TRAFFIC_LIGHT_PADDING_PX = 76

/** macOS overlay: vertical inset applied in Rust (`macos_traffic_lights.rs`). */
export const MAC_TRAFFIC_LIGHT_POSITION_Y = 17
