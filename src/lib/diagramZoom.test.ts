// @vitest-environment happy-dom
import { describe, it, expect, beforeEach } from 'vitest'
import {
  openDiagramZoom,
  closeDiagramZoom,
  zoomIconSvg,
  OVERLAY_CLASS,
} from './diagramZoom'

describe('diagramZoom', () => {
  beforeEach(() => {
    closeDiagramZoom()
    document.body.innerHTML = ''
  })

  it('exports a non-empty magnifier icon', () => {
    expect(zoomIconSvg.length).toBeGreaterThan(0)
    expect(zoomIconSvg).toContain('<svg')
  })

  it('opens overlay with a deep clone for svg sources and rewrites scoped styles', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('id', 'g1')
    svg.setAttribute('viewBox', '0 0 100 50')
    const style = document.createElementNS('http://www.w3.org/2000/svg', 'style')
    style.textContent = '#g1 .node { fill: red }'
    svg.appendChild(style)

    openDiagramZoom(svg as unknown as HTMLElement)

    const overlay = document.querySelector(`.${OVERLAY_CLASS}`)
    expect(overlay).not.toBeNull()
    const cloned = overlay?.querySelector('svg')
    expect(cloned).not.toBeNull()
    expect(cloned?.getAttribute('viewBox')).toBe('0 0 100 50')
    // clone gets a fresh id (source keeps its own)
    expect(cloned?.getAttribute('id')).toMatch(/^diagram-zoom-\d+$/)
    expect(svg.getAttribute('id')).toBe('g1')
    // embedded style selectors are rewritten to the clone id
    expect(cloned?.querySelector('style')?.textContent).toContain('#diagram-zoom-')
    expect(cloned?.querySelector('style')?.textContent).not.toContain('#g1 ')
    expect(cloned?.classList.contains('diagram-zoom-svg')).toBe(true)
  })

  it('renders canvas sources as a png data-url image', () => {
    const canvas = document.createElement('canvas')
    canvas.width = 120
    canvas.height = 60
    const ctx = canvas.getContext('2d')
    ctx?.fillRect(0, 0, 120, 60)

    openDiagramZoom(canvas)

    const overlay = document.querySelector(`.${OVERLAY_CLASS}`)
    const img = overlay?.querySelector('img')
    expect(img).not.toBeNull()
    expect(img?.src.startsWith('data:image/png')).toBe(true)
  })

  it('fits wide diagrams into the viewport on open', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    // happy-dom default viewport: 1024x768 → fit area 921x691.
    svg.setAttribute('viewBox', '0 0 2000 500')
    openDiagramZoom(svg as unknown as HTMLElement)
    const overlay = document.querySelector(`.${OVERLAY_CLASS}`)
    const cloned = overlay?.querySelector('svg')
    expect(cloned?.style.width).toBe('2000px')
    // min(921/2000, 691/500, 1) = 0.4605 → 46%
    expect(overlay?.querySelector('.diagram-zoom-indicator')?.textContent).toBe('46%')
  })

  it('closes on Escape and leaves no overlay behind', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('viewBox', '0 0 10 10')
    openDiagramZoom(svg as unknown as HTMLElement)
    expect(document.querySelector(`.${OVERLAY_CLASS}`)).not.toBeNull()

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(document.querySelector(`.${OVERLAY_CLASS}`)).toBeNull()
  })

  it('captures Escape so other document listeners (e.g. diff maximized) are not triggered', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('viewBox', '0 0 10 10')

    // Simulate an app-level listener registered BEFORE the overlay opens
    // (DiffView / TerminalLiveOutputModal style, bubble phase).
    let escaped = 0
    const spy = (e: KeyboardEvent) => {
      if (e.key === 'Escape') escaped += 1
    }
    document.addEventListener('keydown', spy)

    openDiagramZoom(svg as unknown as HTMLElement)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))

    document.removeEventListener('keydown', spy)
    expect(escaped).toBe(0)
    expect(document.querySelector(`.${OVERLAY_CLASS}`)).toBeNull()
  })

  it('allows only one overlay at a time', () => {
    const a = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    a.setAttribute('viewBox', '0 0 10 10')
    const b = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    b.setAttribute('viewBox', '0 0 20 20')

    openDiagramZoom(a as unknown as HTMLElement)
    openDiagramZoom(b as unknown as HTMLElement)

    const overlays = document.querySelectorAll(`.${OVERLAY_CLASS}`)
    expect(overlays.length).toBe(1)
    expect(overlays[0]?.querySelector('svg')?.getAttribute('viewBox')).toBe('0 0 20 20')
  })
})
