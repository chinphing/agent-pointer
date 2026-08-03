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

/**
 * Chromium falls back per missing glyph, so Latin monospace can lead.
 * Keep CJK faces later in the stack for paste/echo of Chinese.
 */
export const TERMINAL_CJK_FONT_FAMILY_CHROMIUM =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Cascadia Mono", "Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "Noto Sans Mono CJK SC", "Noto Sans CJK SC", monospace'

/**
 * WKWebView/Safari canvas text often does **not** fall back when the leading
 * face lacks CJK (Menlo/SF Mono) — Chinese paste/echo renders as blank boxes
 * while ASCII looks fine. Put CJK-capable faces first on WebKit.
 */
export const TERMINAL_CJK_FONT_FAMILY_WEBKIT =
  '"PingFang SC", "Hiragino Sans GB", "Microsoft YaHei UI", "Microsoft YaHei", "Noto Sans Mono CJK SC", "Noto Sans CJK SC", Menlo, Monaco, Consolas, monospace'

/** @deprecated Prefer {@link terminalFontFamily}; kept for older imports/tests. */
export const TERMINAL_CJK_FONT_FAMILY = TERMINAL_CJK_FONT_FAMILY_WEBKIT

/** True for WKWebView / Safari / WebKitGTK — not Chromium / WebView2. */
export function isWebKitTerminalHost(): boolean {
  if (typeof navigator === 'undefined') return false
  const ua = navigator.userAgent
  return /AppleWebKit/i.test(ua) && !/Chrome\//i.test(ua) && !/Chromium\//i.test(ua)
}

/** Font stack for xterm canvas — WebKit needs CJK faces first. */
export function terminalFontFamily(): string {
  return isWebKitTerminalHost()
    ? TERMINAL_CJK_FONT_FAMILY_WEBKIT
    : TERMINAL_CJK_FONT_FAMILY_CHROMIUM
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
 * Inline styles beat xterm.css (`opacity: 0`) when load order races in release.
 */
export function applyImeFriendlyTextareaStyles(textarea: HTMLTextAreaElement): void {
  const s = textarea.style
  s.setProperty('opacity', '1', 'important')
  s.setProperty('z-index', '1', 'important')
  s.setProperty('color', 'transparent', 'important')
  s.setProperty('-webkit-text-fill-color', 'transparent', 'important')
  s.setProperty('caret-color', 'transparent', 'important')
  s.setProperty('background', 'transparent', 'important')
  s.setProperty('text-shadow', 'none', 'important')
  s.outline = 'none'
  s.border = 'none'
  s.padding = '0'
  s.margin = '0'
  // Keep the field focusable for system IME (some release WKWebView builds
  // treat readOnly / disabled-looking fields as non-IME).
  textarea.readOnly = false
  textarea.disabled = false
}
