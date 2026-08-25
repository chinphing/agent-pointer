// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import { sanitizeSvgMarkup } from './markdownSvg'

describe('sanitizeSvgMarkup (DOMParser)', () => {
  it('accepts a closed diagram whose text contains a raw query-string ampersand', () => {
    const raw = `<svg viewBox="0 0 400 40" xmlns="http://www.w3.org/2000/svg" role="img">
  <text x="10" y="24">spa/.../requestid=903539&sessionkey=abc</text>
</svg>`
    const parsed = sanitizeSvgMarkup(raw)
    expect(parsed.ok).toBe(true)
    if (!parsed.ok) return
    expect(parsed.svg).toContain('&amp;sessionkey')
    const doc = new DOMParser().parseFromString(parsed.svg, 'image/svg+xml')
    expect(doc.querySelector('parsererror')).toBeNull()
    expect(doc.querySelector('text')?.textContent).toContain(
      'requestid=903539&sessionkey=abc'
    )
  })
})
