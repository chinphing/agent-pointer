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
