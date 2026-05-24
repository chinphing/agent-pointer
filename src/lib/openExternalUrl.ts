import { isTauriRuntime } from './runtime'

/** Open http(s) links in the OS default browser (Tauri) or a new tab (Web). */
export async function openExternalUrl(url: string): Promise<void> {
  const trimmed = url.trim()
  if (!trimmed) return
  try {
    const parsed = new URL(trimmed)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return
  } catch {
    return
  }

  if (isTauriRuntime()) {
    const { open } = await import('@tauri-apps/plugin-shell')
    await open(trimmed)
    return
  }
  window.open(trimmed, '_blank', 'noopener,noreferrer')
}
