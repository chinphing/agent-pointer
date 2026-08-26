import { describe, expect, it } from 'vitest'
import {
  ensureBlankLineAfterListBeforeSection,
  parseMarkdown,
} from './markdownConfig'

describe('ensureBlankLineAfterListBeforeSection', () => {
  it('closes the list only when the next line starts with **', () => {
    const src = [
      '调整方案（三档，风险递增）',
      '**档 1: 只归置测试**',
      '- 业务代码零改动',
      '- 只动测试目录',
      '**档 2: 档 1 + 工具脚本分组**',
      '- 入口脚本分组',
      '**档 3: 统一入口收敛**',
      '- CLI 子命令',
    ].join('\n')
    const normalized = ensureBlankLineAfterListBeforeSection(src)
    expect(normalized).toContain('只动测试目录\n\n**档 2:')
    expect(normalized).toContain('入口脚本分组\n\n**档 3:')
    const html = parseMarkdown(src)
    expect(html).not.toMatch(/<li>[^<]*档 [23]:/)
    expect(html).toContain('<strong>档 2:')
    expect(html).toContain('<strong>档 3:')
  })

  it('does not split a plain unindented wrap after a list item', () => {
    const src = '- last bullet\n档 2: title\nstill the same paragraph'
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(src)
  })

  it('inserts only one blank line so later newlines stay as breaks', () => {
    const src = '- last bullet\n**档 2: title**\nstill the same paragraph'
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(
      '- last bullet\n\n**档 2: title**\nstill the same paragraph'
    )
    const html = parseMarkdown(src)
    expect(html).toContain('<p><strong>档 2: title</strong><br>still the same paragraph</p>')
  })

  it('does not rewrite fenced code that looks like a list + bold title', () => {
    const src = ['```md', '- last bullet', '**档 2: still code**', '```'].join('\n')
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(src)
  })

  it('does not insert a blank line before an ATX heading after a list', () => {
    const src = '- done\n## Next'
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(src)
  })
})
