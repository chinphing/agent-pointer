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

  it('maps translated Mermaid groups into root user space before crop', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg') as SVGSVGElement
    svg.setAttribute('viewBox', '0 0 400 500')
    // Identity root CTM; child CTM translates local (0,0) down by 180.
    const identity = {
      a: 1, b: 0, c: 0, d: 1, e: 0, f: 0,
      multiply(other: DOMMatrix) {
        return {
          a: other.a, b: other.b, c: other.c, d: other.d, e: other.e, f: other.f,
          inverse() {
            return {
              a: 1, b: 0, c: 0, d: 1, e: -other.e, f: -other.f,
              multiply(m: DOMMatrix) {
                return {
                  a: m.a, b: m.b, c: m.c, d: m.d,
                  e: m.e + this.e,
                  f: m.f + this.f
                } as DOMMatrix
              }
            } as DOMMatrix
          }
        } as DOMMatrix
      },
      inverse() {
        return this as unknown as DOMMatrix
      }
    } as unknown as DOMMatrix
    svg.getCTM = () => identity
    svg.createSVGPoint = () =>
      ({
        x: 0,
        y: 0,
        matrixTransform(m: DOMMatrix) {
          return { x: this.x + m.e, y: this.y + m.f, matrixTransform() { return this } }
        }
      }) as DOMPoint

    const g = document.createElementNS('http://www.w3.org/2000/svg', 'g') as SVGGraphicsElement
    g.getBBox = () =>
      ({
        x: 10,
        y: 0,
        width: 200,
        height: 120,
        top: 0,
        left: 10,
        right: 210,
        bottom: 120,
        toJSON: () => ({})
      }) as DOMRect
    g.getCTM = () =>
      ({
        a: 1, b: 0, c: 0, d: 1, e: 40, f: 180
      }) as DOMMatrix
    svg.appendChild(g)

    applySvgMountLayout(svg, { cropToContent: true })
    const vb = intrinsicSvgSizeFromViewBox(svg.getAttribute('viewBox'))
    expect(vb).not.toBeNull()
    // Content paints at (50, 180)-(250, 300); pad 16.
    expect(vb!.x).toBe(34)
    expect(vb!.y).toBe(164)
    expect(vb!.width).toBe(232)
    expect(vb!.height).toBe(152)
  })
})
