// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest'
import {
  composerMirrorContentWidth,
  composerMirrorStyleDeclarations,
  composerMirrorText,
  createComposerTextareaMirror
} from './composerTextareaMirror'

describe('composer textarea mirror — text', () => {
  it('adds a zero-width sentinel only where a line box is needed', () => {
    expect(composerMirrorText('')).toBe('\u200b')
    expect(composerMirrorText('a\n')).toBe('a\n\u200b')
    expect(composerMirrorText('a\nb')).toBe('a\nb')
    expect(composerMirrorText('abc')).toBe('abc')
  })

  it('never changes the wrapping of the text it measures', () => {
    for (const value of ['', 'a', 'a\n', 'a\nb', 'x'.repeat(200)]) {
      const measured = composerMirrorText(value)
      expect(measured.startsWith(value)).toBe(true)
      // The sentinel carries no width, so only the extra line box can differ.
      expect(measured.length - value.length).toBeLessThanOrEqual(1)
    }
  })
})

describe('composer textarea mirror — geometry', () => {
  it('measures the padding box minus the textarea padding', () => {
    expect(composerMirrorContentWidth(320, 12, 12)).toBe(296)
    // Never negative, even for a box narrower than its padding.
    expect(composerMirrorContentWidth(10, 12, 12)).toBe(0)
    expect(composerMirrorContentWidth(Number.NaN, 0, 0)).toBe(0)
  })

  it('copies every text-layout property the textarea reports', () => {
    const values: Record<string, string> = {
      'font-family': 'Inter, sans-serif',
      'font-size': '15px',
      'line-height': '20px',
      'padding-top': '8px',
      'padding-left': '12px',
      'white-space': 'pre-wrap',
      'overflow-wrap': 'break-word'
    }
    const declarations = composerMirrorStyleDeclarations({
      getPropertyValue: (property) => values[property] ?? ''
    })
    expect(declarations['font-size']).toBe('15px')
    expect(declarations['line-height']).toBe('20px')
    expect(declarations['padding-left']).toBe('12px')
    expect(declarations['white-space']).toBe('pre-wrap')
    expect(declarations['overflow-wrap']).toBe('break-word')
    // Missing properties are not written as empty strings.
    expect(declarations['letter-spacing']).toBeUndefined()
  })

  it('forces overflow-wrap: break-word when the textarea computes to normal', () => {
    // A textarea that soft-wraps a pasted URL still reports `normal` — the
    // break is engine behaviour. Copying `normal` made the mirror lay the run
    // out on one line and under-count the height.
    const declarations = composerMirrorStyleDeclarations({
      getPropertyValue: (property) => (property === 'overflow-wrap' ? 'normal' : '')
    })
    expect(declarations['overflow-wrap']).toBe('break-word')
  })

  it('keeps a stronger overflow-wrap the stylesheet asks for', () => {
    const declarations = composerMirrorStyleDeclarations({
      getPropertyValue: (property) => (property === 'overflow-wrap' ? 'anywhere' : '')
    })
    // `anywhere` breaks at least as eagerly as `break-word`, so it can only
    // over-count — which is the safe direction.
    expect(declarations['overflow-wrap']).toBe('anywhere')
  })
})

describe('composer textarea mirror — DOM adapter', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  function textarea(): HTMLTextAreaElement {
    const el = document.createElement('textarea')
    el.className = 'composer-textarea'
    document.body.appendChild(el)
    return el
  }

  function mirrorNode(): HTMLDivElement | null {
    return document.querySelector<HTMLDivElement>('[data-composer-textarea-mirror="1"]')
  }

  it('creates the mirror lazily, only on the first measure', () => {
    const el = textarea()
    const mirror = createComposerTextareaMirror(el)
    expect(mirrorNode()).toBeNull()

    mirror.measure('hello')
    expect(mirrorNode()).not.toBeNull()
  })

  it('reports null when the mirror cannot be laid out, so the caller falls back', () => {
    // happy-dom does no layout: every box is 0 × 0. That is exactly the
    // "no measurement available" contract — the destructive probe must run.
    const el = textarea()
    const mirror = createComposerTextareaMirror(el)
    expect(mirror.measure('hello')).toBeNull()
    expect(mirror.measure('a\nb')).toBeNull()
  })

  it('disposes its node and can measure again afterwards', () => {
    const el = textarea()
    const mirror = createComposerTextareaMirror(el)

    mirror.measure('hello')
    const node = mirrorNode()
    expect(node).not.toBeNull()

    mirror.dispose()
    expect(mirrorNode()).toBeNull()

    mirror.measure('hello')
    expect(mirrorNode()).not.toBeNull()
  })

  it('survives the mirror being removed from the document', () => {
    const el = textarea()
    const mirror = createComposerTextareaMirror(el)

    mirror.measure('hello')
    mirrorNode()?.remove()

    // Detached → null (fall back), and the next call re-creates it.
    expect(mirror.measure('hello')).toBeNull()
    expect(mirrorNode()).toBeNull()
    mirror.measure('hello')
    expect(mirrorNode()).not.toBeNull()
  })
})

/**
 * The mirror's whole job is "same inline size, same wrapping as the textarea".
 * happy-dom does no layout, so these cases drive a fake one: the mirror node
 * reports `ceil(characters / charsPerLine)` lines for the width it was given.
 * That keeps the assertions on the width the mirror is *fed* — the thing that
 * was wrong — instead of on an engine's line breaking.
 */
describe('composer textarea mirror — inline size', () => {
  const CHAR_PX = 10
  const LINE_PX = 20
  const PADDING_Y = 16

  afterEach(() => {
    document.body.innerHTML = ''
  })

  /** Textarea whose box metrics the test controls (happy-dom has no layout). */
  function sizedTextarea(box: { clientWidth: number; offsetWidth: number }) {
    const el = document.createElement('textarea')
    el.className = 'composer-textarea'
    el.style.paddingTop = '8px'
    el.style.paddingBottom = '8px'
    el.style.paddingLeft = '12px'
    el.style.paddingRight = '12px'
    el.style.borderLeftWidth = '1px'
    el.style.borderRightWidth = '1px'
    document.body.appendChild(el)
    Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => box.clientWidth })
    Object.defineProperty(el, 'offsetWidth', { configurable: true, get: () => box.offsetWidth })
    return el
  }

  function mirrorNode(): HTMLDivElement | null {
    return document.querySelector<HTMLDivElement>('[data-composer-textarea-mirror="1"]')
  }

  /** Fake layout for the mirror node: CHAR_PX per character, LINE_PX per line. */
  function attachLayout(node: HTMLDivElement) {
    Object.defineProperty(node, 'getBoundingClientRect', {
      configurable: true,
      value: () => {
        const width = Number.parseFloat(node.style.width) || 0
        const perLine = Math.max(1, Math.floor(width / CHAR_PX))
        const text = node.textContent ?? ''
        const lines = Math.max(1, Math.ceil(text.length / perLine))
        const height = lines * LINE_PX + PADDING_Y
        return {
          width,
          height,
          top: 0,
          left: 0,
          right: width,
          bottom: height,
          x: 0,
          y: 0,
          toJSON: () => ({})
        }
      }
    })
  }

  /** The first measure creates the node; attach the fake layout to it. */
  function readyMirror(el: HTMLTextAreaElement) {
    const mirror = createComposerTextareaMirror(el)
    mirror.measure('x')
    attachLayout(mirrorNode()!)
    return mirror
  }

  it('sizes the mirror from clientWidth, not from offsetWidth', () => {
    // Border box 337 = 320 padding box + 15 scrollbar gutter + 2 border.
    const el = sizedTextarea({ clientWidth: 320, offsetWidth: 337 })
    const mirror = readyMirror(el)

    // 60 chars at 296px → 29 chars/line → 3 lines. Feeding offsetWidth would
    // have given 311px → 31 chars/line → 2 lines: the line that got clipped.
    expect(mirror.measure('x'.repeat(60))).toBe(3 * LINE_PX + PADDING_Y)
    expect(mirrorNode()?.style.width).toBe('296px')
  })

  it('re-reads the geometry when a scrollbar changes clientWidth', () => {
    const box = { clientWidth: 335, offsetWidth: 337 }
    const el = sizedTextarea(box)
    const mirror = readyMirror(el)

    // No scrollbar: 335 − 24 = 311 → 31 chars/line → 2 lines.
    expect(mirror.measure('x'.repeat(60))).toBe(2 * LINE_PX + PADDING_Y)

    // A classic (space-taking) scrollbar appears: same border box, 15px less
    // text width. A cache keyed on offsetWidth would hand back the stale 311px.
    box.clientWidth = 320
    expect(mirror.measure('x'.repeat(60))).toBe(3 * LINE_PX + PADDING_Y)
    expect(mirrorNode()?.style.width).toBe('296px')
  })

  it('sets overflow-wrap on the mirror node itself', () => {
    const el = sizedTextarea({ clientWidth: 320, offsetWidth: 337 })
    const mirror = readyMirror(el)
    mirror.measure('x'.repeat(60))
    expect(mirrorNode()?.style.getPropertyValue('overflow-wrap')).toBe('break-word')
  })

  it('reports no measurement at zero width instead of one line per character', () => {
    // Display-none / not-yet-laid-out textarea: a zero-width mirror would count
    // every character as a line, and the caller would grow the box to the cap.
    const el = sizedTextarea({ clientWidth: 0, offsetWidth: 0 })
    const mirror = readyMirror(el)
    expect(mirror.measure('x'.repeat(60))).toBeNull()
  })
})
