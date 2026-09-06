import { describe, expect, it } from 'vitest'
import {
  ensureBlankLinesAroundHtmlTables,
  parseMarkdown,
  wrapBareHtmlTables
} from './markdownConfig'
import { isHtmlFenceLang, sanitizeHtmlFence } from './markdownHtml'

describe('ensureBlankLinesAroundHtmlTables', () => {
  it('inserts blank lines so Markdown resumes after HTML tables', () => {
    const src = [
      '## 报销进度汇总',
      '<table><tr><td>a</td></tr></table>',
      '## 待补充清单',
      '<table><tr><td>b</td></tr></table>',
      '## 当前进展',
      '正文',
      '',
      '**加粗**',
      '',
      '1. 一项',
      '',
      '[链接](https://example.com)',
    ].join('\n')
    const normalized = ensureBlankLinesAroundHtmlTables(src)
    expect(normalized).toContain('</table>\n\n## 待补充清单')
    expect(normalized).toContain('</table>\n\n## 当前进展')
    const html = parseMarkdown(src)
    expect(html).toContain('<h2>报销进度汇总</h2>')
    expect(html).toContain('<h2>待补充清单</h2>')
    expect(html).toContain('<h2>当前进展</h2>')
    expect(html).toContain('<strong>加粗</strong>')
    expect(html).toContain('<ol>')
    expect(html).toContain('href="https://example.com"')
    expect(html).not.toMatch(/>\s*## 待补充清单/)
  })
})

describe('wrapBareHtmlTables', () => {
  it('wraps a bare HTML table', () => {
    const raw =
      '<table style="width:100%"><tr><th style="width:30%">检查项</th><th>结果</th></tr>' +
      '<tr><td>审批表</td><td style="color:#1a7f37;">通过</td></tr></table>'
    const out = wrapBareHtmlTables(raw)
    expect(out.startsWith('<div class="table-wrapper"><table')).toBe(true)
    expect(out.endsWith('</table></div>')).toBe(true)
    expect(out).toContain('style="color:#1a7f37;"')
  })

  it('does not double-wrap GFM table-wrapper output', () => {
    const gfm =
      '<div class="table-wrapper"><table><thead><tr><th>A</th></tr></thead>' +
      '<tbody><tr><td>1</td></tr></tbody></table></div>'
    expect(wrapBareHtmlTables(gfm)).toBe(gfm)
  })

  it('wraps nested tables as a single unit', () => {
    const raw =
      '<table border="1"><tr><td>outer</td><td>' +
      '<table border="1"><tr><td>inner</td></tr></table>' +
      '</td></tr></table>'
    const out = wrapBareHtmlTables(raw)
    // The wrapper must enclose the ENTIRE outer table, not stop at the inner </table>.
    expect(out).toBe(`<div class="table-wrapper">${raw}</div>`)
    expect(out.match(/table-wrapper/g)?.length).toBe(1)
  })

  it('handles multiple sibling tables with nesting', () => {
    const raw =
      '<table><tr><td><table><tr><td>a</td></tr></table></td></tr></table>' +
      '<p>between</p>' +
      '<table><tr><td>b</td></tr></table>'
    const out = wrapBareHtmlTables(raw)
    expect(out.match(/table-wrapper/g)?.length).toBe(2)
    expect(out).toContain('<p>between</p>')
  })
})

describe('html fence', () => {
  it('recognizes html/htm langs', () => {
    expect(isHtmlFenceLang('html')).toBe(true)
    expect(isHtmlFenceLang('HTM')).toBe(true)
    expect(isHtmlFenceLang('bash')).toBe(false)
  })

  it('strips script and event handlers', () => {
    const cleaned = sanitizeHtmlFence(
      '<table><tr><td onclick="alert(1)">x</td></tr></table><script>evil()</script>'
    )
    expect(cleaned).toContain('<table>')
    expect(cleaned).not.toContain('script')
    expect(cleaned).not.toContain('onclick')
  })

  it('renders ```html tables instead of a code card', () => {
    const src = [
      '```html',
      '<table>',
      '  <colgroup>',
      '    <col style="width:5%">',
      '    <col style="width:30%">',
      '  </colgroup>',
      '  <tr><th>#</th><th>Description</th></tr>',
      '  <tr><td>1</td><td>赴北京参加会议</td></tr>',
      '</table>',
      '```',
    ].join('\n')
    const html = parseMarkdown(src)
    expect(html).toContain('class="table-wrapper"')
    expect(html).toContain('<colgroup>')
    expect(html).toContain('width:30%')
    expect(html).toContain('赴北京参加会议')
    expect(html).not.toContain('code-block')
    expect(html).not.toContain('language-html')
  })
})

describe('parseMarkdown HTML tables', () => {
  it('keeps bare HTML tables renderable with wrapper and inline styles', () => {
    const src = `<table border="1" style="width:100%; border-collapse:collapse;">
  <tr>
    <th style="width:30%;">检查项</th>
    <th style="width:15%;">结果</th>
    <th>说明</th>
  </tr>
  <tr>
    <td>审批表</td>
    <td style="color:#1a7f37;">通过</td>
    <td>已上传，内容完整</td>
  </tr>
</table>`
    const html = parseMarkdown(src)
    expect(html).toContain('class="table-wrapper"')
    expect(html).toContain('<table')
    expect(html).toContain('width:30%')
    expect(html).toContain('color:#1a7f37')
    expect(html).toContain('审批表')
    expect(html.match(/table-wrapper/g)?.length).toBe(1)
  })

  it('still wraps GFM pipe tables once', () => {
    const html = parseMarkdown('| A | B |\n| --- | --- |\n| 1 | 2 |')
    expect(html).toContain('class="table-wrapper"')
    expect(html.match(/table-wrapper/g)?.length).toBe(1)
  })
})
