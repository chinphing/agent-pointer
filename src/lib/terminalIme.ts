/**
 * WebKit (Tauri WKWebView / Safari) IME helpers for xterm.
 *
 * Chromium activates IME on xterm's opacity:0 helper textarea; WebKit does not,
 * and also drops some insertText commits while a key is held. These helpers
 * restore CJK composition / paste delivery without changing Linux Chromium web.
 */

const PURE_PRINTABLE_ASCII = /^[\x20-\x7E]+$/

/** No onData after a non-empty compositionend → xterm read an empty substring. */
const COMMIT_FALLBACK_MS = 80
/** onData this close before compositionend means xterm already flushed. */
const SYNC_FLUSH_MS = 30

/** Explicit CJK fallbacks: macOS monospace stacks omit Chinese glyphs in WKWebView. */
export const TERMINAL_CJK_FONT_FAMILY =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC", monospace'

/** True for WKWebView / Safari / WebKitGTK — not Chromium / WebView2. */
export function isWebKitTerminalHost(): boolean {
  if (typeof navigator === 'undefined') return false
  const ua = navigator.userAgent
  return /AppleWebKit/i.test(ua) && !/Chrome\//i.test(ua) && !/Chromium\//i.test(ua)
}

/**
 * Keys that must stay with the browser/IME. Returning false from
 * attachCustomKeyEventHandler skips xterm's keydown processing.
 */
export function shouldDeferKeyToIme(event: KeyboardEvent): boolean {
  return (
    event.isComposing ||
    event.keyCode === 229 ||
    event.keyCode === 0 ||
    event.key === 'Process' ||
    event.key === 'Dead'
  )
}

/** Raw pinyin buffer from mid-composition input-source switch (spaces between syllables). */
export function isAbandonedImeAsciiBuffer(data: string): boolean {
  return PURE_PRINTABLE_ASCII.test(data) && data.includes(' ') && data.trim() !== ''
}

export type TerminalImeGuard = {
  observeKeyEvent: (event: KeyboardEvent) => void
  attach: (textarea: HTMLTextAreaElement) => void
  detach: () => void
  filterData: (data: string) => string
}

/**
 * Forwards insertText / composition commits that xterm 6 drops on WebKit.
 * `send` must write to the PTY (same path as terminal.onData).
 */
export function createTerminalImeGuard(send: (data: string) => void): TerminalImeGuard {
  let textarea: HTMLTextAreaElement | null = null
  let keydownConsumedByIme = false
  let keypressFired = false
  let composing = false
  let lastDataAt = Number.NEGATIVE_INFINITY
  let pendingStrip: string | null = null
  let fallbackTimer: ReturnType<typeof setTimeout> | null = null

  const cancelFallback = () => {
    if (fallbackTimer !== null) {
      clearTimeout(fallbackTimer)
      fallbackTimer = null
    }
  }

  const handleCompositionStart = () => {
    composing = true
  }

  const handleInput = (event: Event) => {
    const ev = event as InputEvent
    if (ev.inputType !== 'insertText' || !ev.data || ev.isComposing) return
    // xterm already forwarded this keystroke (or will via keypress).
    if (ev.defaultPrevented || keypressFired || keydownConsumedByIme) return
    cancelFallback()
    send(ev.data)
  }

  const handleCompositionEnd = (event: Event) => {
    composing = false
    const data = (event as CompositionEvent).data
    cancelFallback()
    pendingStrip = data && isAbandonedImeAsciiBuffer(data) ? data : null
    if (!data) return
    if (performance.now() - lastDataAt < SYNC_FLUSH_MS) return
    const payload = pendingStrip ? data.replace(/\s+/g, '') : data
    fallbackTimer = setTimeout(() => {
      fallbackTimer = null
      pendingStrip = null
      send(payload)
    }, COMMIT_FALLBACK_MS)
  }

  const clearPasteResidue = () => {
    // xterm paste only stopPropagation's; WebKit still inserts into the helper
    // textarea. Clear residue so a later 229-diff does not re-send it.
    if (!textarea || composing) return
    queueMicrotask(() => {
      if (!textarea || composing) return
      if (textarea.value) textarea.value = ''
    })
  }

  return {
    observeKeyEvent(event) {
      if (event.type === 'keydown') {
        keydownConsumedByIme = event.keyCode === 229
        keypressFired = false
      } else if (event.type === 'keypress') {
        keypressFired = true
      }
    },
    attach(target) {
      textarea = target
      target.addEventListener('compositionstart', handleCompositionStart)
      target.addEventListener('input', handleInput)
      target.addEventListener('compositionend', handleCompositionEnd)
      target.addEventListener('paste', clearPasteResidue)
    },
    detach() {
      cancelFallback()
      textarea?.removeEventListener('compositionstart', handleCompositionStart)
      textarea?.removeEventListener('input', handleInput)
      textarea?.removeEventListener('compositionend', handleCompositionEnd)
      textarea?.removeEventListener('paste', clearPasteResidue)
      textarea = null
      composing = false
    },
    filterData(data) {
      if (data.startsWith('\x1b')) return data
      lastDataAt = performance.now()
      cancelFallback()
      const pending = pendingStrip
      pendingStrip = null
      if (pending !== null && data.startsWith(pending)) {
        return pending.replace(/\s+/g, '') + data.slice(pending.length)
      }
      return data
    }
  }
}

/**
 * Make xterm's helper textarea engine-visible for WebKit IME without painting
 * text/caret. Position/size stay under xterm's cursor sync.
 */
export function applyImeFriendlyTextareaStyles(textarea: HTMLTextAreaElement): void {
  const s = textarea.style
  s.opacity = '1'
  s.color = 'transparent'
  s.background = 'transparent'
  s.caretColor = 'transparent'
  s.textShadow = 'none'
  s.outline = 'none'
  s.border = 'none'
  s.padding = '0'
  s.margin = '0'
}
