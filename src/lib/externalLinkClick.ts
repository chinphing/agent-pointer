import { openExternalUrl } from './openExternalUrl'

function isExternalWebLink(href: string): boolean {
  const trimmed = href.trim()
  if (!trimmed || trimmed.startsWith('#') || trimmed.startsWith('javascript:')) return false
  try {
    const parsed = new URL(trimmed, window.location.href)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

function isSameAppOrigin(href: string): boolean {
  try {
    const parsed = new URL(href, window.location.href)
    return parsed.origin === window.location.origin
  } catch {
    return false
  }
}

/**
 * Capture http(s) anchor clicks app-wide and open via OS browser (Tauri) or a new tab (Web).
 * Same-origin links stay in-app. Modifier/middle-click left to the browser.
 */
export function installExternalLinkClickHandler(): void {
  document.addEventListener(
    'click',
    (e) => {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) {
        return
      }
      const target = e.target
      if (!(target instanceof Element)) return
      const anchor = target.closest('a')
      if (!anchor || anchor.hasAttribute('download')) return
      const href = anchor.getAttribute('href')
      if (!href || !isExternalWebLink(href) || isSameAppOrigin(href)) return
      e.preventDefault()
      e.stopPropagation()
      void openExternalUrl(href)
    },
    true
  )
}
