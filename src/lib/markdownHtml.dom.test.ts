// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import { sanitizeHtmlFence } from './markdownHtml'

describe('sanitizeHtmlFence (DOMParser path)', () => {
  it('strips nested obfuscated script tags', () => {
    const cleaned = sanitizeHtmlFence(
      '<table><tr><td>safe</td></tr></table><scr<script>ipt>alert(1)</scr</script>ipt>'
    )
    expect(cleaned).toContain('<table>')
    expect(cleaned).toContain('safe')
    expect(cleaned).not.toContain('<script')
  })

  it('strips event handlers with > in quoted values', () => {
    const cleaned = sanitizeHtmlFence(
      '<table><tr><td onclick="alert(1>2)">x</td></tr></table>'
    )
    expect(cleaned).toContain('<table>')
    expect(cleaned).not.toContain('onclick')
  })

  it('neutralizes javascript: URLs', () => {
    const cleaned = sanitizeHtmlFence(
      '<a href="javascript:alert(1)">click</a>'
    )
    expect(cleaned).not.toContain('javascript:')
    expect(cleaned).toContain('href="#"')
  })

  it('strips iframe and form tags', () => {
    const cleaned = sanitizeHtmlFence(
      '<p>text</p><iframe src="evil"></iframe><form action="evil"><input></form>'
    )
    expect(cleaned).toContain('text')
    expect(cleaned).not.toContain('<iframe')
    expect(cleaned).not.toContain('<form')
  })

  it('preserves safe table structure', () => {
    const cleaned = sanitizeHtmlFence(
      '<table border="1"><tr><td style="color:red">cell</td></tr></table>'
    )
    expect(cleaned).toContain('<table')
    expect(cleaned).toContain('cell')
    expect(cleaned).toContain('color:red')
  })
})
