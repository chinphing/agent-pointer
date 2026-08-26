import { describe, expect, it } from 'vitest'
import {
  ensureBlankLineAfterListBeforeSection,
  parseMarkdown,
} from './markdownConfig'

describe('ensureBlankLineAfterListBeforeSection', () => {
  it('closes the list so the next unindented line is not inside the last item', () => {
    const src = [
      '调整方案（三档，风险递增）',
      '档 1: 只归置测试',
      '- 业务代码零改动',
      '- 只动测试目录',
      '档 2: 档 1 + 工具脚本分组',
      '- 入口脚本分组',
      '档 3: 统一入口收敛',
      '- CLI 子命令',
    ].join('\n')
    const normalized = ensureBlankLineAfterListBeforeSection(src)
    expect(normalized).toContain('只动测试目录\n\n档 2:')
    expect(normalized).toContain('入口脚本分组\n\n档 3:')
    const html = parseMarkdown(src)
    expect(html).not.toMatch(/<li>[^<]*档 [123]:/)
    expect(html).toContain('档 1: 只归置测试')
    expect(html).toMatch(/<p>档 2:/)
    expect(html).toMatch(/<p>档 3:/)
  })

  it('inserts only one blank line so later newlines stay as breaks', () => {
    const src = '- last bullet\n档 2: title\nstill the same paragraph'
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(
      '- last bullet\n\n档 2: title\nstill the same paragraph'
    )
    const html = parseMarkdown(src)
    expect(html).toContain('<p>档 2: title<br>still the same paragraph</p>')
  })

  it('keeps indented continuation inside the list', () => {
    const src = '- outer\n  nested detail\n档 2: next'
    const normalized = ensureBlankLineAfterListBeforeSection(src)
    expect(normalized).toContain('nested detail\n\n档 2:')
    const html = parseMarkdown(src)
    expect(html).toMatch(/<li>[\s\S]*nested detail/)
  })

  it('does not rewrite fenced code that looks like a list + title', () => {
    const src = ['```md', '- last bullet', '档 2: still code', '```'].join('\n')
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(src)
  })

  it('inserts a blank line before an ATX heading after a list', () => {
    const src = '- done\n## Next'
    expect(ensureBlankLineAfterListBeforeSection(src)).toContain('done\n\n## Next')
    const html = parseMarkdown(src)
    expect(html).toContain('<h2>Next</h2>')
    expect(html).not.toMatch(/<li>[^<]*## Next/)
  })
})
