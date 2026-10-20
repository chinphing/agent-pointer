// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import {
  COMPOSER_TEXTAREA_FRAME_PAINT_ID,
  applyComposerTextareaFramePaint,
  composerFrameCssValue,
  composerTextareaFrameHtml
} from './composerTextareaFrame'

describe('composer textarea frame document', () => {
  const html = composerTextareaFrameHtml()

  it('is a textarea-only document with a transparent page', () => {
    expect(html).toContain('<textarea class="composer-textarea"')
    expect(html).toContain('background: transparent')
    expect(html).toContain(`id="${COMPOSER_TEXTAREA_FRAME_PAINT_ID}"`)
    expect(html).not.toContain('<script')
  })

  it('strips style-sheet breakout characters from copied paint values', () => {
    expect(composerFrameCssValue('rgb(1, 2, 3)')).toBe('rgb(1, 2, 3)')
    expect(composerFrameCssValue('a{color:red}</style>')).toBe('acolor:red/style')
  })
})

describe('composer textarea frame paint', () => {
  it('writes color, placeholder, and the wide-layout padding', () => {
    const doc = document.implementation.createHTMLDocument('frame')
    const style = doc.createElement('style')
    style.id = COMPOSER_TEXTAREA_FRAME_PAINT_ID
    doc.head.appendChild(style)
    applyComposerTextareaFramePaint(doc, {
      color: 'rgb(1, 2, 3)',
      placeholderColor: 'rgb(4, 5, 6)',
      backgroundColor: 'rgb(7, 8, 9)',
      fontFamily: 'sans-serif',
      colorScheme: 'dark',
      desktop: true
    })
    expect(style.textContent).toContain('color:rgb(1, 2, 3)')
    expect(style.textContent).toContain('rgb(4, 5, 6)')
    expect(style.textContent).toContain('background:rgb(7, 8, 9)')
    expect(style.textContent).toContain('padding:3px 12px 8px')
    expect(doc.documentElement.style.colorScheme).toBe('dark')
  })

  it('warns once when the paint style is missing', () => {
    const warnings: unknown[][] = []
    const original = console.warn
    console.warn = (...args: unknown[]) => {
      warnings.push(args)
    }
    try {
      const doc = document.implementation.createHTMLDocument('frame')
      applyComposerTextareaFramePaint(doc, {
        color: 'black',
        placeholderColor: 'gray',
        backgroundColor: 'white',
        fontFamily: 'sans-serif',
        colorScheme: 'light',
        desktop: false
      })
    } finally {
      console.warn = original
    }
    expect(warnings).toHaveLength(1)
    expect(String(warnings[0][0])).toContain('paint style missing')
  })
})
