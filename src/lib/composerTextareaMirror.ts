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
 * mirror) include padding and exclude the border.
 *
 * Two things decide where the text wraps, and both have to be right or the
 * mirror under-counts lines (the box stops short and the last line is clipped
 * at the bottom edge):
 *
 * 1. **Inline size.** The text lays out in the *padding box minus the scrollbar
 *    gutter* — that is exactly `clientWidth`. `offsetWidth` is the border box
 *    and still contains the gutter, so once `overflow-y: auto` shows a classic
 *    (space-taking) scrollbar the mirror was a scrollbar wider than the
 *    textarea, wrapped later, and reported one line too few.
 * 2. **Break opportunity inside a run with no spaces.** A textarea soft-wraps a
 *    pasted URL or a long CJK run at the padding edge as an engine default,
 *    while `getComputedStyle` reports `overflow-wrap: normal` for it. Copying
 *    that `normal` let the mirror lay the whole run out on one line. The mirror
 *    therefore forces `overflow-wrap: break-word` (see
 *    `composerMirrorStyleDeclarations`); wrapping *more* eagerly than the
 *    textarea can cost one extra line, wrapping less loses a visible one.
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

/**
 * `overflow-wrap` the mirror always uses.
 *
 * `break-word` matches how a textarea breaks a run that has no break
 * opportunity of its own (pasted URL, unspaced CJK) at the padding edge. It is
 * forced rather than copied because the computed value of a textarea that does
 * soft-wrap such runs is `normal` — the wrapping lives in engine behaviour, not
 * in the cascaded value.
 */
const MIRROR_OVERFLOW_WRAP = 'break-word'

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

/**
 * The inline size the text gets, in px.
 *
 * `paddingBoxWidth` must be the textarea's `clientWidth`: the padding box with
 * any scrollbar gutter already removed. `offsetWidth` would include the gutter
 * and make the mirror wrap too late (see the module comment).
 */
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
  // Always explicit, never `normal`: a textarea breaks an unspaced run at the
  // padding edge even though its computed `overflow-wrap` says `normal`.
  // `anywhere` is at least as eager as `break-word`, so keep it when the CSS
  // asks for it; anything else (including `normal`) becomes `break-word`.
  const declaredWrap = declarations['overflow-wrap']
  declarations['overflow-wrap'] =
    declaredWrap === 'anywhere' ? declaredWrap : MIRROR_OVERFLOW_WRAP
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

  function syncGeometry(target: HTMLDivElement, force: boolean): number {
    // `clientWidth` is the padding box minus the scrollbar gutter — the inline
    // size the text actually lays out in. `offsetWidth` (border box) still
    // counts the gutter, which made the mirror wrap later than the textarea
    // once `overflow-y: auto` showed a classic scrollbar. Re-reading the
    // computed style on every keystroke would flush style for the textarea, so
    // it is cached — keyed on `clientWidth`, because a scrollbar appearing or
    // disappearing changes the width the text gets without changing the border
    // box, and a stale key would hand back geometry for the wrong wrap width.
    const paddingBoxWidth = el.clientWidth
    const key = `${paddingBoxWidth}`
    if (!force && geometry && geometry.key === key) {
      applyWidth(target, geometry.width)
      return geometry.width
    }
    // The textarea may live in the composer frame. Read styles from that
    // document; the parent's getComputedStyle does not resolve them.
    const view = el.ownerDocument?.defaultView
    const computed = view && typeof view.getComputedStyle === 'function' ? view.getComputedStyle(el) : null
    if (!computed) {
      const width = Math.max(0, paddingBoxWidth)
      applyWidth(target, width)
      return width
    }
    const declarations = composerMirrorStyleDeclarations(computed)
    const width = composerMirrorContentWidth(
      paddingBoxWidth,
      readPx(computed, 'padding-left'),
      readPx(computed, 'padding-right')
    )
    geometry = { key, width, declarations }
    applyWidth(target, width)
    for (const [property, value] of Object.entries(declarations)) {
      target.style.setProperty(property, value)
    }
    return width
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
      const width = syncGeometry(target, force)
      // A zero-width mirror does not lay text out the way the textarea does —
      // it would report one line per character. Report "no measurement" so the
      // caller falls back instead of growing the box to the cap.
      if (!(width > 0)) return null
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
