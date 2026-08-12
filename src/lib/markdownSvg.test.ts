import { describe, expect, it } from 'vitest'
import {
  STREAMING_SVG_HOST_HTML,
  STREAMING_SVG_STUB,
  parseMarkdown,
  stabilizeStreamingSvgFences,
} from './markdownConfig'
import {
  sanitizeSvgMarkup,
  tryParseSvgFence,
  intrinsicSvgSizeFromViewBox,
  applySvgMountLayout,
  fitSvgViewBoxToAttributedContent,
  normalizeFlowArrows,
  expandSvgArrowMarkers,
} from './markdownSvg'

describe('sanitizeSvgMarkup', () => {
  it('accepts a simple flowchart svg', () => {
    const raw = `<svg viewBox="0 0 100 40" xmlns="http://www.w3.org/2000/svg">
  <rect x="10" y="10" width="80" height="20" rx="4" fill="#E6F1FB" stroke="#185FA5"/>
  <text x="50" y="24" text-anchor="middle" font-size="10" fill="#0C447C">A</text>
</svg>`
    const parsed = sanitizeSvgMarkup(raw)
    expect(parsed.ok).toBe(true)
    if (parsed.ok) {
      expect(parsed.svg).toContain('<svg')
      expect(parsed.svg).toContain('</svg>')
      expect(parsed.svg).toContain('viewBox')
    }
  })

  it('strips script and event handlers', () => {
    const raw = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
  <script>alert(1)</script>
  <rect width="10" height="10" onclick="alert(1)" href="javascript:alert(1)"/>
</svg>`
    const parsed = sanitizeSvgMarkup(raw)
    expect(parsed.ok).toBe(true)
    if (parsed.ok) {
      expect(parsed.svg.toLowerCase()).not.toContain('<script')
      expect(parsed.svg.toLowerCase()).not.toContain('onclick')
      expect(parsed.svg.toLowerCase()).not.toContain('javascript:')
    }
  })

  it('rejects non-svg markup', () => {
    expect(tryParseSvgFence('<div>hi</div>').ok).toBe(false)
    expect(tryParseSvgFence('').ok).toBe(false)
  })

  it('allows fragment href on use', () => {
    const raw = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
  <defs><circle id="c" r="2"/></defs>
  <use href="#c" x="5" y="5"/>
</svg>`
    const parsed = sanitizeSvgMarkup(raw)
    expect(parsed.ok).toBe(true)
    if (parsed.ok) expect(parsed.svg).toContain('href="#c"')
  })
})

describe('intrinsicSvgSizeFromViewBox / applySvgMountLayout', () => {
  it('parses viewBox width/height', () => {
    expect(intrinsicSvgSizeFromViewBox('0 0 760 300')).toEqual({
      x: 0,
      y: 0,
      width: 760,
      height: 300,
    })
    expect(intrinsicSvgSizeFromViewBox('0,0,100,40')).toEqual({
      x: 0,
      y: 0,
      width: 100,
      height: 40,
    })
    expect(intrinsicSvgSizeFromViewBox('bad')).toBeNull()
    expect(intrinsicSvgSizeFromViewBox(null)).toBeNull()
  })

  it('sizes the root from viewBox and clears percentage width crush', () => {
    const style: Record<string, string> = {}
    const attrs = new Map<string, string>([
      ['viewBox', '0 0 760 300'],
      ['width', '100%'],
    ])
    const svg = {
      getAttribute: (name: string) => attrs.get(name) ?? null,
      setAttribute: (name: string, value: string) => {
        attrs.set(name, value)
      },
      style,
      querySelectorAll: () => [],
    } as unknown as SVGElement
    applySvgMountLayout(svg)
    expect(attrs.get('width')).toBe('760')
    expect(attrs.get('height')).toBe('300')
    expect(style.width).toBe('760px')
    expect(style.maxWidth).toBe('none')
    expect(attrs.get('overflow')).toBe('visible')
  })

  it('expands undersized viewBox so bottom content is not clipped', () => {
    const raw = `<svg viewBox="0 0 760 260" xmlns="http://www.w3.org/2000/svg">
  <rect x="260" y="248" width="240" height="34" fill="#F5EEF9"/>
  <text x="380" y="269" text-anchor="middle" font-size="12">pending_summary</text>
  <text x="20" y="300" font-size="11">footer note</text>
</svg>`
    const fitted = fitSvgViewBoxToAttributedContent(raw)
    const vb = intrinsicSvgSizeFromViewBox(
      fitted.match(/viewBox="([^"]+)"/)?.[1]
    )
    expect(vb).not.toBeNull()
    expect(vb!.height).toBeGreaterThan(260)
    expect(vb!.y + vb!.height).toBeGreaterThanOrEqual(300)

    const sanitized = sanitizeSvgMarkup(raw)
    expect(sanitized.ok).toBe(true)
    if (sanitized.ok) {
      const svb = intrinsicSvgSizeFromViewBox(
        sanitized.svg.match(/viewBox="([^"]+)"/)?.[1]
      )
      expect(svb!.height).toBeGreaterThan(260)
    }
  })
})

describe('parseMarkdown svg fences', () => {
  it('emits an md-svg host for valid svg', () => {
    const src =
      '```svg\n' +
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>\n' +
      '```'
    const html = parseMarkdown(src)
    expect(html).toContain('class="md-svg group"')
    expect(html).toContain('data-svg-config=')
    expect(html).toContain('md-svg-frame')
    expect(html).not.toContain('code-block')
  })

  it('emits an invalid host for incomplete svg', () => {
    const src = '```svg\n<svg viewBox="0 0 10 10">\n```'
    const html = parseMarkdown(src)
    expect(html).toContain('md-svg--invalid')
    expect(html).toContain('data-svg-config=')
  })

  it('keeps ordinary code fences as code-block', () => {
    const html = parseMarkdown('```bash\necho hi\n```')
    expect(html).toContain('code-block')
    expect(html).not.toContain('md-svg')
  })

  it('stabilizeStreamingSvgFences stubs only incomplete fences', () => {
    const open = stabilizeStreamingSvgFences(
      'Intro\n\n```svg\n<svg viewBox="0 0 10 10"><rect\n'
    )
    const closed =
      'Intro\n\n```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>\n```\n'
    const closedStab = stabilizeStreamingSvgFences(closed)
    expect(open).toContain(STREAMING_SVG_STUB)
    expect(closedStab).not.toContain(STREAMING_SVG_STUB)
    expect(closedStab).toContain('<rect width="10" height="10"/>')
  })

  it('streamingSvgs keeps pending HTML while fence is open', () => {
    const prefix = '流程如下：\n\n'
    const htmlOpen = parseMarkdown(
      prefix + '```svg\n<svg viewBox="0 0 10 10"><rect\n',
      { streamingSvgs: true }
    )
    expect(htmlOpen).toContain(STREAMING_SVG_HOST_HTML.trim())
    expect(htmlOpen).toContain('图示生成中…')
  })

  it('streamingSvgs mounts a closed fence before the reply finishes', () => {
    const src =
      '流程如下：\n\n```svg\n' +
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>\n' +
      '```\n\n后面还有说明文字仍在流式输出…\n'
    const html = parseMarkdown(src, { streamingSvgs: true })
    expect(html).toContain('class="md-svg group"')
    expect(html).not.toContain('md-svg--pending')
    expect(html).not.toContain('图示生成中…')
    expect(html).toContain('data-svg-config=')
    expect(html).toContain('后面还有说明文字')
  })
})

describe('normalizeFlowArrows', () => {
  it('leaves curved paths intact when the arrow tip is attached', () => {
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">' +
      '<defs><marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">' +
      '<path d="M2 1 L8 5 L2 9"/></marker></defs>' +
      '<rect x="10" y="10" width="40" height="20" fill="#eee"/>' +
      '<rect x="10" y="60" width="40" height="20" fill="#eee"/>' +
      '<path d="M30 30 C 30 45, 30 45, 30 60" fill="none" stroke="#333" marker-end="url(#ar)"/>' +
      '</svg>'
    const parsed = sanitizeSvgMarkup(svg)
    expect(parsed.ok).toBe(true)
    if (!parsed.ok) return
    // Tip sits on the bottom rect edge -> snap is not triggered; curve survives.
    expect(parsed.svg).toContain('C ')
    // Arrowhead is materialized as an explicit polygon.
    expect(parsed.svg).toContain('<polygon')
  })

  const PROBLEM_SVG = `<svg viewBox="-16 -16 652 292" xmlns="http://www.w3.org/2000/svg">
  <rect x="15" y="20" width="180" height="52" rx="8" fill="#FFF4E0" stroke="#B7791F"/>
  <rect x="235" y="20" width="180" height="52" rx="8" fill="#E6F1FB" stroke="#185FA5"/>
  <rect x="455" y="20" width="150" height="52" rx="8" fill="#E1F5EE" stroke="#0F6E56"/>
  <rect x="235" y="100" width="180" height="52" rx="8" fill="#E1F5EE" stroke="#0F6E56"/>
  <rect x="15" y="190" width="180" height="52" rx="8" fill="#E6F1FB" stroke="#185FA5"/>
  <defs>
    <marker id="a" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M2 1L8 5L2 9" fill="none" stroke="#5F5E5A"/>
    </marker>
  </defs>
  <path d="M195 46 L235 46" stroke="#5F5E5A" stroke-width="1.5" marker-end="url(#a)"/>
  <path d="M415 46 L455 46" stroke="#5F5E5A" stroke-width="1.5" marker-end="url(#a)"/>
  <path d="M530 72 L530 100 L325 100 L325 78" stroke="#5F5E5A" stroke-width="1.5" fill="none" marker-end="url(#a)"/>
  <path d="M325 152 L325 190 L105 190 L105 170" stroke="#5F5E5A" stroke-width="1.5" fill="none" marker-end="url(#a)"/>
</svg>`

  it('snaps floating arrow tips onto the nearest rect edge and expands markers', () => {
    const out = normalizeFlowArrows(PROBLEM_SVG)
    expect(out).not.toContain('marker-end')
    expect(out).toContain('<polygon points=')
    // 6px float above rect ② bottom edge (y=72) → snapped onto it
    expect(out).toContain('L325 100 L325 72')
    // 20px float above rect ⑤ top edge (y=190) → snapped, prev re-pointed outside
    expect(out).toContain('L105 180 L105 190')
    expect(out).not.toContain('L105 170')
    // Already-touching horizontal arrows keep their geometry
    expect(out).toContain('M195 46 L235 46')
    expect(out).toContain('M415 46 L455 46')
  })

  it('keeps already-touching arrow tips unchanged', () => {
    const svg = `<svg viewBox="0 0 300 100" xmlns="http://www.w3.org/2000/svg">
  <rect x="10" y="20" width="100" height="40"/>
  <rect x="150" y="20" width="100" height="40"/>
  <defs>
    <marker id="a" viewBox="0 0 12 12" refX="10" refY="6" markerWidth="10" markerHeight="10" orient="auto" markerUnits="userSpaceOnUse">
      <path d="M1 1 L10 6 L1 11 Z" fill="#5F5E5A"/>
    </marker>
  </defs>
  <path d="M110 40 L150 40" stroke="#5F5E5A" stroke-width="1.5" marker-end="url(#a)"/>
</svg>`
    const out = normalizeFlowArrows(svg)
    expect(out).toContain('d="M110 40 L150 40"')
    expect(out).not.toContain('marker-end')
    expect(out).toContain('<polygon points=')
  })

  it('expands a horizontal arrow marker to a polygon whose tip is the path end', () => {
    const svg = `<svg viewBox="0 0 300 100" xmlns="http://www.w3.org/2000/svg">
  <rect x="150" y="20" width="100" height="40"/>
  <defs>
    <marker id="a" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M2 1L8 5L2 9" fill="none" stroke="#5F5E5A"/>
    </marker>
  </defs>
  <path d="M110 40 L150 40" stroke="#5F5E5A" stroke-width="1.5" marker-end="url(#a)"/>
</svg>`
    const out = expandSvgArrowMarkers(svg)
    const m = out.match(/<polygon points="([^"]+)"/)
    expect(m).not.toBeNull()
    const pts = m![1]!.split(' ').map(p => p.split(',').map(Number))
    // Tip (refX aligns to path end) lands exactly on the endpoint.
    expect(
      pts.some(([x, y]) => Math.abs(x! - 150) < 0.01 && Math.abs(y! - 40) < 0.01)
    ).toBe(true)
    // Tail distance from tip = (8-2) * strokeWidth(1.5) * markerWidth(6) / viewBox(10) = 5.4
    const tailXs = pts.filter(([x]) => x! < 150)
    for (const [x] of tailXs) {
      expect(Math.abs(x! - (150 - 5.4))).toBeLessThan(0.1)
    }
  })

  it('sanitize integrates normalization end-to-end', () => {
    const parsed = sanitizeSvgMarkup(PROBLEM_SVG)
    expect(parsed.ok).toBe(true)
    if (parsed.ok) {
      expect(parsed.svg).not.toContain('marker-end')
      expect(parsed.svg).toContain('<polygon')
      expect(parsed.svg).toContain('L325 72')
      expect(parsed.svg).toContain('L105 190')
    }
  })
})
