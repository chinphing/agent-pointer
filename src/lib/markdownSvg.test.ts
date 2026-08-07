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
