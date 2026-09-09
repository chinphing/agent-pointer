import { describe, expect, it } from 'vitest'
import {
  ensureBlankLineAfterBlockquote,
  parseMarkdown,
} from './markdownConfig'

function blockquoteInner(html: string): string {
  const m = html.match(/<blockquote>([\s\S]*?)<\/blockquote>/)
  return m?.[1] ?? ''
}

describe('ensureBlankLineAfterBlockquote', () => {
  it('closes a quote when the next line omits > (no author blank line)', () => {
    const src = [
      '3. 本单效果：',
      '   > **住宿支付凭证**：住宿发票.pdf',
      '   > 疑似对公付款（开票方：北京展览馆宾馆有限公司）',
      '按这个改吗？改完我重跑预览给您看。',
    ].join('\n')
    const normalized = ensureBlankLineAfterBlockquote(src)
    expect(normalized).toContain(
      '疑似对公付款（开票方：北京展览馆宾馆有限公司）\n\n按这个改吗？'
    )
    const html = parseMarkdown(src)
    const quote = blockquoteInner(html)
    expect(quote).toContain('疑似对公付款')
    expect(quote).not.toContain('按这个改吗')
    expect(html).toContain('按这个改吗？改完我重跑预览给您看。')
  })

  it('keeps consecutive > lines in one blockquote', () => {
    const src = '> line one\n> line two\n> line three'
    expect(ensureBlankLineAfterBlockquote(src)).toBe(src)
    const html = parseMarkdown(src)
    expect(html.match(/<blockquote>/g)?.length ?? 0).toBe(1)
  })

  it('does not rewrite when a blank line already ends the quote', () => {
    const src = '> quoted\n\nplain after'
    expect(ensureBlankLineAfterBlockquote(src)).toBe(src)
  })

  it('does not rewrite fenced code that contains > lines', () => {
    const src = ['```md', '> still code', 'plain', '```'].join('\n')
    expect(ensureBlankLineAfterBlockquote(src)).toBe(src)
  })

  it('leaves soft-wrap-without-> as a new paragraph (product choice)', () => {
    const src = '> quoted sentence\ncontinues without marker'
    expect(ensureBlankLineAfterBlockquote(src)).toBe(
      '> quoted sentence\n\ncontinues without marker'
    )
    const html = parseMarkdown(src)
    expect(blockquoteInner(html)).not.toContain('continues without marker')
    expect(html).toContain('<p>continues without marker</p>')
  })
})
