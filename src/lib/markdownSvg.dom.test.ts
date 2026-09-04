// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import {
  applySvgMountLayout,
  intrinsicSvgSizeFromViewBox,
  sanitizeSvgMarkup,
} from './markdownSvg'

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

describe('applySvgMountLayout cropToContent', () => {
  it('crops mermaid-style empty viewBox padding', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('viewBox', '0 0 800 600')
    svg.setAttribute('width', '100%')
    svg.style.maxWidth = '800px'
    const plate = document.createElementNS('http://www.w3.org/2000/svg', 'rect')
    plate.setAttribute('x', '0')
    plate.setAttribute('y', '0')
    plate.setAttribute('width', '800')
    plate.setAttribute('height', '600')
    plate.getBBox = () =>
      ({
        x: 0,
        y: 0,
        width: 800,
        height: 600,
        top: 0,
        left: 0,
        right: 800,
        bottom: 600,
        toJSON: () => ({}),
      }) as DOMRect
    const g = document.createElementNS('http://www.w3.org/2000/svg', 'g')
    g.getBBox = () =>
      ({
        x: 120,
        y: 200,
        width: 400,
        height: 220,
        top: 200,
        left: 120,
        right: 520,
        bottom: 420,
        toJSON: () => ({}),
      }) as DOMRect
    svg.appendChild(plate)
    svg.appendChild(g)
    applySvgMountLayout(svg, { cropToContent: true })
    const vb = intrinsicSvgSizeFromViewBox(svg.getAttribute('viewBox'))
    expect(vb).not.toBeNull()
    expect(vb!.x).toBe(104)
    expect(vb!.y).toBe(184)
    expect(vb!.width).toBe(432)
    expect(vb!.height).toBe(252)
    expect(svg.style.maxWidth).toBe('432px')
  })
})
