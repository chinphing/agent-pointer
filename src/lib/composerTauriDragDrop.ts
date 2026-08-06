/**
 * Single window-level Tauri drag-drop listener for Composer.
 *
 * Why not register per Composer instance:
 * - `onDragDropEvent` is webview/window scoped; each listener receives every drop.
 * - Composer remounts (welcome ↔ chat, compact dock) can race async setup and leak
 *   listeners when unlisten is still null at onUnmounted — one file then appears N times
 *   in the shared `composerAttachments` store.
 * - Tauri may also emit duplicate drop payloads in rapid succession (upstream race).
 *
 * Callers: `acquireComposerTauriDragDrop` — one shared listener, latest subscriber handles
 * hover/drop; drop paths are deduped within a short window.
 */

import { isTauriRuntime } from './runtime'

export const COMPOSER_DROP_DEDUP_MS = 800

export type ComposerTauriDragHandlers = {
  onHover: (active: boolean) => void
  onDrop: (paths: string[]) => void | Promise<void>
}

export function normalizeComposerDropPath(path: string): string {
  return path.replace(/\\/g, '/').replace(/\/+$/, '')
}

export function composerDropPathsKey(paths: readonly string[]): string {
  return [...paths].map(normalizeComposerDropPath).filter(Boolean).sort().join('\0')
}

export function uniqueComposerDropPaths(paths: readonly string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const path of paths) {
    const trimmed = path.trim()
    if (!trimmed) continue
    const key = normalizeComposerDropPath(trimmed)
    if (seen.has(key)) continue
    seen.add(key)
    out.push(trimmed)
  }
  return out
}

export function shouldAcceptComposerDrop(
  paths: readonly string[],
  last: { key: string; at: number } | null,
  now = Date.now(),
  windowMs = COMPOSER_DROP_DEDUP_MS
): { accept: boolean; next: { key: string; at: number } } {
  const key = composerDropPathsKey(paths)
  if (!key) {
    return { accept: false, next: last ?? { key: '', at: now } }
  }
  if (last && last.key === key && now - last.at < windowMs) {
    return { accept: false, next: last }
  }
  return { accept: true, next: { key, at: now } }
}

const handlerStack: ComposerTauriDragHandlers[] = []
let unlistenNative: (() => void) | null = null
let setupPromise: Promise<void> | null = null
let lastAcceptedDrop: { key: string; at: number } | null = null

function activeHandler(): ComposerTauriDragHandlers | null {
  return handlerStack[handlerStack.length - 1] ?? null
}

async function ensureNativeListener(): Promise<void> {
  if (unlistenNative || !isTauriRuntime()) return
  if (setupPromise) {
    await setupPromise
    return
  }
  setupPromise = (async () => {
    try {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview')
      if (handlerStack.length === 0) return
      const webview = getCurrentWebview()
      const stop = await webview.onDragDropEvent(async event => {
        const payload = event.payload
        const handler = activeHandler()
        if (!handler) return
        if (payload.type === 'enter' || payload.type === 'over') {
          handler.onHover(true)
          return
        }
        if (payload.type === 'drop') {
          handler.onHover(false)
          const paths = uniqueComposerDropPaths(payload.paths ?? [])
          if (!paths.length) return
          const decision = shouldAcceptComposerDrop(paths, lastAcceptedDrop)
          if (!decision.accept) {
            console.info('[composer-drag-drop] ignore duplicate native drop', paths)
            return
          }
          lastAcceptedDrop = decision.next
          console.info('[composer-drag-drop] native drop', paths)
          await handler.onDrop(paths)
          return
        }
        handler.onHover(false)
      })
      // Remount race: all Composers may have released while we awaited registration.
      if (handlerStack.length === 0) {
        stop()
        return
      }
      unlistenNative = stop
      console.info('[composer-drag-drop] native listener ready')
    } catch (err) {
      console.warn('tauri composer drag-drop listener failed', err)
    } finally {
      setupPromise = null
    }
  })()
  await setupPromise
}

function releaseNativeListenerIfIdle() {
  if (handlerStack.length > 0) return
  unlistenNative?.()
  unlistenNative = null
  setupPromise = null
  lastAcceptedDrop = null
}

/**
 * Register for window-level OS file drops. Returns a disposer (idempotent).
 * No-op outside Tauri.
 */
export async function acquireComposerTauriDragDrop(
  handlers: ComposerTauriDragHandlers
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {}

  handlerStack.push(handlers)
  await ensureNativeListener()

  let released = false
  return () => {
    if (released) return
    released = true
    const idx = handlerStack.lastIndexOf(handlers)
    if (idx >= 0) handlerStack.splice(idx, 1)
    releaseNativeListenerIfIdle()
  }
}

/** Test helper — clears singleton state between unit tests. */
export function resetComposerTauriDragDropForTests() {
  handlerStack.length = 0
  unlistenNative?.()
  unlistenNative = null
  setupPromise = null
  lastAcceptedDrop = null
}
