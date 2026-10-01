/**
 * Clipboard write with a fallback, for the dev-only render-perf HUD.
 *
 * Why it is shaped like this:
 *
 * - **Primary is the async Clipboard API**, the only path that needs no DOM — but
 *   it is unavailable outside a secure context and rejects when the document is
 *   not focused, both of which can happen in the macOS Tauri webview the HUD runs
 *   in.
 * - **Fallback is a temporary hidden textarea plus `document.execCommand('copy')`**,
 *   the pre-Clipboard-API route, which still works in those webviews. The textarea
 *   is removed again in a `finally`, so nothing is left in the DOM.
 * - **It never throws and always reports.** A silent failure is worse than a
 *   visible one here: the point of the feature is that the numbers can be pasted
 *   instead of screenshotted, so the caller has to know when they cannot be.
 */

/**
 * How long a copy confirmation stays on screen before the affordance comes back.
 * Lives here (not in the HUD) so the constant and the component test that advances
 * the clock by it cannot drift apart.
 */
export const CLIPBOARD_FEEDBACK_MS = 1800

/** `true` when the text reached the clipboard through either path. */
export async function writeClipboardText(text: string): Promise<boolean> {
  if (await writeViaClipboardApi(text)) return true
  return writeViaTextarea(text)
}

async function writeViaClipboardApi(text: string): Promise<boolean> {
  try {
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard
    if (!clipboard || typeof clipboard.writeText !== 'function') return false
    await clipboard.writeText(text)
    return true
  } catch {
    // Missing (insecure context) or rejected (document not focused): the caller
    // falls back, and the reason is not actionable from here.
    return false
  }
}

function writeViaTextarea(text: string): boolean {
  if (typeof document === 'undefined') return false
  const execCommand = document.execCommand
  if (typeof execCommand !== 'function') return false

  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.setAttribute('aria-hidden', 'true')
  textarea.setAttribute('tabindex', '-1')
  // Parked off-screen rather than `display: none`: a hidden element cannot be
  // selected, and selecting it is what `execCommand('copy')` copies.
  textarea.style.position = 'fixed'
  textarea.style.top = '0'
  textarea.style.left = '-9999px'
  textarea.style.opacity = '0'
  textarea.style.pointerEvents = 'none'
  document.body.appendChild(textarea)

  try {
    textarea.select()
    textarea.setSelectionRange(0, text.length)
    return execCommand.call(document, 'copy') === true
  } catch {
    return false
  } finally {
    textarea.remove()
  }
}
