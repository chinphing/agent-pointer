import { describe, expect, it } from 'vitest'
import {
  STREAMING_SVG_HOST_HTML,
  STREAMING_SVG_STUB,
  parseMarkdown,
  stabilizeStreamingSvgFences,
} from './markdownConfig'
import { sanitizeSvgMarkup, tryParseSvgFence } from './markdownSvg'

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

  it('stabilizeStreamingSvgFences collapses growing svg to a stub', () => {
    const a = stabilizeStreamingSvgFences(
      'Intro\n\n```svg\n<svg viewBox="0 0 10 10"><rect\n'
    )
    const b = stabilizeStreamingSvgFences(
      'Intro\n\n```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>\n```\n'
    )
    expect(a).toContain(STREAMING_SVG_STUB)
    expect(b).toContain(STREAMING_SVG_STUB)
    expect(a.replace(/\n+$/, '')).toBe(b.replace(/\n+$/, ''))
  })

  it('streamingSvgs parse keeps identical HTML while fence grows', () => {
    const prefix = '流程如下：\n\n'
    const htmlA = parseMarkdown(
      prefix + '```svg\n<svg viewBox="0 0 10 10"><rect\n',
      { streamingSvgs: true }
    )
    const htmlB = parseMarkdown(
      prefix +
        '```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>\n```\n',
      { streamingSvgs: true }
    )
    expect(htmlA).toBe(htmlB)
    expect(htmlA).toContain(STREAMING_SVG_HOST_HTML.trim())
    expect(htmlA).toContain('图示生成中…')
  })
})
