import { describe, expect, it } from 'vitest'
import { parseMarkdown } from './markdownConfig'
import {
  MAX_MATH_SOURCE_CHARS,
  findBlockMath,
  findBlockMathStart,
  findInlineDisplayMath,
  findInlineMath,
  mathHtmlCacheSize,
  renderMathHtml,
} from './markdownMath'

const BLOCK_OPEN = '<span class="md-math md-math--block">'

describe('findInlineMath', () => {
  it('matches a simple inline formula', () => {
    const found = findInlineMath('$S_{ij}$ 表示注意力权重')
    expect(found).not.toBeNull()
    expect(found!.source).toBe('S_{ij}')
    expect(found!.raw).toBe('$S_{ij}$')
  })

  it('rejects an opening delimiter followed by whitespace', () => {
    expect(findInlineMath('$ x$')).toBeNull()
  })

  it('rejects a closing delimiter preceded by whitespace', () => {
    expect(findInlineMath('$x $')).toBeNull()
  })

  it('rejects a closing delimiter followed by a digit', () => {
    expect(findInlineMath('$5$10')).toBeNull()
  })

  it('returns null when the delimiter is never closed', () => {
    expect(findInlineMath('$100 元')).toBeNull()
    expect(findInlineMath('$')).toBeNull()
  })

  it('never spans a line break', () => {
    expect(findInlineMath('$a\nb$')).toBeNull()
  })

  it('ignores an escaped dollar when looking for the closer', () => {
    const found = findInlineMath('$a \\$ b$')
    expect(found).not.toBeNull()
    expect(found!.source).toBe('a \\$ b')
  })

  it('does not match the $$ display form', () => {
    expect(findInlineMath('$$x$$')).toBeNull()
  })

  it('refuses an oversized formula', () => {
    const huge = '$' + 'x'.repeat(MAX_MATH_SOURCE_CHARS + 1) + '$'
    expect(findInlineMath(huge)).toBeNull()
  })
})

describe('findInlineDisplayMath', () => {
  it('matches a single-line $$…$$', () => {
    const found = findInlineDisplayMath('$$e^{1}=2.7183$$ 之后还有文字')
    expect(found).not.toBeNull()
    expect(found!.source).toBe('e^{1}=2.7183')
    expect(found!.raw).toBe('$$e^{1}=2.7183$$')
  })

  it('returns null while the closing $$ has not streamed in', () => {
    expect(findInlineDisplayMath('$$e^{1}')).toBeNull()
    expect(findInlineDisplayMath('$$')).toBeNull()
  })

  it('does not span a line break', () => {
    expect(findInlineDisplayMath('$$a\nb$$')).toBeNull()
  })
})

describe('findBlockMath', () => {
  it('matches a multi-line block', () => {
    const found = findBlockMath('$$\na + b\n$$')
    expect(found).not.toBeNull()
    expect(found!.source).toBe('a + b')
    expect(found!.raw).toBe('$$\na + b\n$$')
  })

  it('matches a block whose content starts on the opening line', () => {
    const src =
      '$$X=\\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}\\quad\n' +
      'W_V=\\begin{bmatrix}1&0\\\\1&1\\end{bmatrix}$$'
    const found = findBlockMath(src)
    expect(found).not.toBeNull()
    expect(found!.raw).toBe(src)
    expect(found!.source).toContain('W_V=')
  })

  it('stops at the first line ending with $$', () => {
    const found = findBlockMath('$$a$$\n$$b$$\n')
    expect(found!.raw).toBe('$$a$$')
  })

  it('returns null for an unclosed block (streaming)', () => {
    expect(findBlockMath('$$\n\\frac{1}{2}\n')).toBeNull()
    expect(findBlockMath('$$x$$ 后面还有文字\n')).toBeNull()
  })

  it('requires the delimiter at a line start with at most 3 spaces', () => {
    expect(findBlockMath('    $$x$$')).toBeNull()
    expect(findBlockMath('文本 $$x$$')).toBeNull()
  })
})

describe('findBlockMathStart', () => {
  it('reports the offset of a real block start', () => {
    expect(findBlockMathStart('文字\n$$x$$')).toBe(3)
  })

  it('reports -1 for a $$ that cannot open a block', () => {
    expect(findBlockMathStart('价格 $$100 和 $200')).toBe(-1)
    expect(findBlockMathStart('$$x$$ 后面还有文字\n')).toBe(-1)
  })
})

describe('renderMathHtml memo', () => {
  it('caches one entry per (display, source) pair', () => {
    const source = 'memo_' + Math.random().toString(36).slice(2)
    const before = mathHtmlCacheSize()
    const first = renderMathHtml(source, 'inline')
    expect(mathHtmlCacheSize()).toBe(before + 1)
    const second = renderMathHtml(source, 'inline')
    expect(second).toBe(first)
    expect(mathHtmlCacheSize()).toBe(before + 1)
    // Same source in display mode is a different render.
    renderMathHtml(source, 'display')
    expect(mathHtmlCacheSize()).toBe(before + 2)
  })
})

describe('parseMarkdown math', () => {
  it('renders inline $…$ with the stable md-math class', () => {
    const html = parseMarkdown('打分 $S = QK^\\top$ 表示相似度')
    expect(html).toContain('class="md-math"')
    expect(html).not.toContain('md-math--block')
    expect(html).toContain('class="katex"')
    expect(html).toContain('打分')
    expect(html).toContain('表示相似度')
  })

  it('renders a $$…$$ block as a block-level md-math host', () => {
    const html = parseMarkdown('缩放：\n\n$$\n\\frac{QK^\\top}{\\sqrt{d_k}}\n$$\n')
    expect(html).toContain(BLOCK_OPEN)
    expect(html).not.toContain('<p><span class="md-math md-math--block"')
    expect(html).toContain('缩放')
  })

  it('renders a single-line $$…$$ that a paragraph swallowed', () => {
    const html = parseMarkdown('公式 $$e^{1}=2.7183$$ 如下')
    expect(html).toContain(BLOCK_OPEN)
    expect(html).toContain('公式')
    expect(html).toContain('如下')
  })

  it('keeps a block whose content starts on the opening line', () => {
    const src =
      '$$X=\\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}\\quad\n' +
      'W_V=\\begin{bmatrix}1&0\\\\1&1\\end{bmatrix}$$'
    const html = parseMarkdown(src)
    expect(html).toContain(BLOCK_OPEN)
    expect(html).toContain('katex-display')
  })

  it('renders inline math inside table cells', () => {
    const html = parseMarkdown('| 名称 | 公式 |\n| --- | --- |\n| 点积 | $e = q \\cdot k$ |\n')
    expect(html).toContain('md-math')
    expect(html).toContain('点积')
  })

  it('leaves money amounts as plain text', () => {
    for (const src of [
      '价格是 $100 元。',
      '合计 $100 到 $200 元。',
      '花费 $5 and $10 today',
      'It costs $100.',
    ]) {
      const html = parseMarkdown(src)
      expect(html).not.toContain('md-math')
      expect(html).toContain('$')
    }
  })

  it('does not parse math inside fenced code', () => {
    const html = parseMarkdown('```js\nconst a = $x$ + $y$\n```')
    expect(html).not.toContain('md-math')
    expect(html).toContain('code-block')
    expect(html).toContain('$x$')
  })

  it('does not parse math inside inline code', () => {
    const html = parseMarkdown('用 `$x$` 表示占位符')
    expect(html).not.toContain('md-math')
    expect(html).toContain('<code>$x$</code>')
  })

  it('leaves an escaped dollar as literal text', () => {
    const html = parseMarkdown('转义 \\$x$ 不触发公式')
    expect(html).not.toContain('md-math')
    expect(html).toContain('$x$')
  })

  it('keeps an unclosed $ as text and does not swallow the rest', () => {
    const html = parseMarkdown('未闭合 $x 后面还有文字，以及 **加粗**。')
    expect(html).not.toContain('md-math')
    expect(html).toContain('未闭合')
    expect(html).toContain('后面还有文字')
    expect(html).toContain('<strong>加粗</strong>')
  })

  it('keeps a half-streamed $$ block as text', () => {
    const html = parseMarkdown('公式：$$\n\\frac{1}{2}\n')
    expect(html).not.toContain('md-math')
    expect(html).toContain('公式：')
    expect(html).toContain('\\frac{1}{2}')
  })

  it('survives an invalid formula without breaking the rest of the reply', () => {
    const html = parseMarkdown('坏公式 $\\frac{1}{$ 后续文字')
    expect(html).toContain('md-math')
    expect(html).toContain('katex-error')
    expect(html).toContain('后续文字')
  })

  it('leaves chartjs fences on the chart path', () => {
    const src =
      '```chartjs\n{"type":"bar","data":{"labels":["A"],"datasets":[{"label":"n","data":[1]}]}}\n```'
    const html = parseMarkdown(src)
    expect(html).toContain('md-chart')
    expect(html).not.toContain('md-math')
  })
})
