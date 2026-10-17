/**
 * Non-destructive content-height measurement for the composer textarea.
 *
 * The JS auto-grow fallback (see `composerTextareaResize`) needs the height the
 * *content* would occupy, independent of the box the textarea currently has.
 * `scrollHeight` cannot answer that while the box is taller than the content —
 * it clamps to `clientHeight`. The old answer was a destructive probe: write
 * `height: <min>`, read `scrollHeight`, write the real height back. Both writes
 * mark the textarea style-dirty, so a delete that leaves the height unchanged
 * still paid style recalc + a second layout of the composer → pane → transcript
 * chain.
 *
 * Instead, lay the same text out in a hidden mirror box that copies the
 * textarea's text-layout inputs (font, padding, wrapping, letter/word spacing,
 * line height) and read *that*. The textarea is then only written when its
 * height actually changes.
 *
 * The mirror is a `div` with `position: absolute` (out of flow),
 * `visibility: hidden` (no paint) and `contain: layout style paint` (its layout
 * cannot propagate to the rest of the document). It deliberately does **not**
 * use `contain: size`: size containment makes a box size as if it had no
 * contents, i.e. always 0 — the one number being measured.
 *
 * Measurement semantics match the probe it replaces: `scrollHeight` (and this
 * mirror) include padding and exclude the border, and the probe measured with
 * `overflow-y: hidden`, so no scrollbar gutter is part of the inline size.
 */

/** Text-layout inputs copied from the textarea onto the mirror. */
export const COMPOSER_MIRROR_TEXT_PROPERTIES = [
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'font-stretch',
  'font-variant',
  'font-feature-settings',
  'font-kerning',
  'font-optical-sizing',
  'letter-spacing',
  'word-spacing',
  'line-height',
  'text-indent',
  'text-transform',
  'text-align',
  'text-rendering',
  'text-size-adjust',
  'tab-size',
  'white-space',
  'overflow-wrap',
  'word-break',
  'hyphens',
  'direction',
  'unicode-bidi',
  'padding-top',
  'padding-right',
  'padding-bottom',
  'padding-left'
] as const

/** Zero-width space: forces a line box without contributing any width. */
const MIRROR_LINE_SENTINEL = '\u200b'

/** The slice of a computed style the mirror needs. */
export interface ComposerMirrorStyleSource {
  getPropertyValue(property: string): string
}

/**
 * Text to lay out for `value`.
 *
 * A textarea shows a caret line after a trailing newline, and one line even
 * when empty — an empty `div` has no line box at all. Both cases need a
 * zero-width sentinel to materialise that last line, and it must be zero-width
 * so it never changes where the text wraps.
 */
export function composerMirrorText(value: string): string {
  return value === '' || value.endsWith('\n') ? `${value}${MIRROR_LINE_SENTINEL}` : value
}

/** The inline size the text gets, in px (`clientWidth` is the padding box). */
export function composerMirrorContentWidth(
  paddingBoxWidth: number,
  paddingLeft: number,
  paddingRight: number
): number {
  const width = paddingBoxWidth - paddingLeft - paddingRight
  return Number.isFinite(width) && width > 0 ? width : 0
}

/** Computed text-layout declarations for the mirror. */
export function composerMirrorStyleDeclarations(
  source: ComposerMirrorStyleSource
): Record<string, string> {
  const declarations: Record<string, string> = {}
  for (const property of COMPOSER_MIRROR_TEXT_PROPERTIES) {
    const value = source.getPropertyValue(property)
    if (value) declarations[property] = value
  }
  return declarations
}

export interface ComposerTextareaMirror {
  /**
   * Height the text would occupy at the textarea's current inline size, in px
   * (includes padding, excludes border) — or `null` when the mirror cannot be
   * laid out, in which case the caller must fall back to the destructive probe.
   *
   * `force` re-reads the textarea's computed style (width change / theme font
   * change), which is otherwise cached per inline size.
   */
  measure(value: string, force?: boolean): number | null
  dispose(): void
}

/** Geometry that only changes when the textarea is resized or restyled. */
interface MirrorGeometry {
  key: string
  width: number
  declarations: Record<string, string>
}

function readPx(style: ComposerMirrorStyleSource, property: string): number {
  const value = Number.parseFloat(style.getPropertyValue(property))
  return Number.isFinite(value) ? value : 0
}

/**
 * Create a measurer bound to `el`. The mirror node is created on the first
 * `measure` call, so engines with native `field-sizing` never pay for it.
 */
export function createComposerTextareaMirror(
  el: HTMLTextAreaElement
): ComposerTextareaMirror {
  let node: HTMLDivElement | null = null
  let geometry: MirrorGeometry | null = null

  function ensureNode(): HTMLDivElement | null {
    if (node) return node
    if (typeof document === 'undefined' || !document.body) return null
    const created = document.createElement('div')
    created.dataset.composerTextareaMirror = '1'
    created.setAttribute('aria-hidden', 'true')
    const style = created.style
    style.position = 'absolute'
    style.top = '0'
    style.left = '-10000px'
    style.visibility = 'hidden'
    style.pointerEvents = 'none'
    style.zIndex = '-1'
    // Content-box + the textarea's own paddings → the measured height is the
    // text block plus padding, exactly like `scrollHeight`.
    style.boxSizing = 'content-box'
    style.margin = '0'
    style.border = '0'
    style.minHeight = '0'
    style.maxHeight = 'none'
    style.overflow = 'visible'
    // Keep the measurement subtree's layout from propagating (no `size` — see
    // the module comment).
    style.setProperty('contain', 'layout style paint')
    document.body.appendChild(created)
    node = created
    return node
  }

  function applyWidth(target: HTMLDivElement, width: number): void {
    const next = `${width}px`
    if (target.style.width === next) return
    target.style.width = next
  }

  function syncGeometry(target: HTMLDivElement, force: boolean): void {
    // `offsetWidth` is the border box; the mirror's inline size is the content
    // box, i.e. border box minus border minus padding. The probe measured with
    // `overflow-y: hidden`, so no scrollbar gutter is part of it — `offsetWidth`
    // already includes that gutter, which is what we want here. Re-reading the
    // computed style on every keystroke would flush style for the textarea, so
    // it is cached per border-box width.
    const borderBoxWidth = el.offsetWidth
    const key = `${borderBoxWidth}`
    if (!force && geometry && geometry.key === key) {
      applyWidth(target, geometry.width)
      return
    }
    const computed = typeof getComputedStyle === 'function' ? getComputedStyle(el) : null
    if (!computed) {
      applyWidth(target, Math.max(0, borderBoxWidth))
      return
    }
    const declarations = composerMirrorStyleDeclarations(computed)
    const borderWidth =
      readPx(computed, 'border-left-width') + readPx(computed, 'border-right-width')
    const width = composerMirrorContentWidth(
      borderBoxWidth - borderWidth,
      readPx(computed, 'padding-left'),
      readPx(computed, 'padding-right')
    )
    geometry = { key, width, declarations }
    applyWidth(target, width)
    for (const [property, value] of Object.entries(declarations)) {
      target.style.setProperty(property, value)
    }
  }

  return {
    measure(value: string, force = false): number | null {
      const target = ensureNode()
      if (!target || !target.isConnected) {
        // Detached (or never attached) mirror: nothing to measure.
        if (target) {
          node = null
          target.remove()
        }
        return null
      }
      syncGeometry(target, force)
      target.textContent = composerMirrorText(value)
      const height = target.getBoundingClientRect().height
      if (!Number.isFinite(height) || height <= 0) return null
      // `scrollHeight` (what the probe read) is an integer; round so the two
      // measurement paths produce the same numbers.
      return Math.round(height)
    },
    dispose(): void {
      node?.remove()
      node = null
      geometry = null
    }
  }
}
