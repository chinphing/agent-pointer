import { describe, expect, it } from 'vitest'
import { parseMarkdown } from './markdownConfig'

describe('markdown paragraph hierarchy', () => {
  it('marks a lone bold paragraph as a section title', () => {
    const html = parseMarkdown('**簇 A：住宿费入行口径**')
    expect(html).toContain('<p class="md-section-title"><strong>簇 A：住宿费入行口径</strong></p>')
  })

  it('splits **lead** — body onto its own lead line', () => {
    const html = parseMarkdown('**A1. 842668 曾燕** — 归档住宿=900')
    expect(html).toContain('<span class="md-lead"><strong>A1. 842668 曾燕</strong></span>')
    expect(html).toContain('— 归档住宿=900')
    expect(html).not.toContain('md-section-title')
  })

  it('keeps an inline bold label on the same line', () => {
    const html = parseMarkdown('**口径提醒：** 所有金额差 = detail_1')
    expect(html).toContain('<p><strong>口径提醒：</strong> 所有金额差 = detail_1</p>')
    expect(html).not.toContain('md-lead')
    expect(html).not.toContain('md-section-title')
  })

  it('does not lift mid-sentence bold', () => {
    const html = parseMarkdown('短回复里的 **中间加粗** 不应拆行。')
    expect(html).toContain('<p>短回复里的 <strong>中间加粗</strong> 不应拆行。</p>')
    expect(html).not.toContain('md-lead')
  })
})
