import { describe, expect, it } from 'vitest'
import {
  filePreviewSearchParts,
  findFilePreviewMatches,
  highlightCodeFenceHtml,
  tokenizeCodeLine
} from './workspaceFilePreview'

describe('workspace file preview tokenizer', () => {
  it('classifies keywords, strings, numbers and trailing comments', () => {
    const tokens = tokenizeCodeLine('const answer = "ok" + 42 // note')
    expect(tokens.filter(token => token.kind !== 'plain')).toEqual([
      { text: 'const', kind: 'keyword' },
      { text: '"ok"', kind: 'string' },
      { text: '42', kind: 'number' },
      { text: '// note', kind: 'comment' }
    ])
  })

  it('keeps empty lines visible', () => {
    expect(tokenizeCodeLine('')).toEqual([{ text: '\u00A0', kind: 'plain' }])
  })

  it('renders fenced HTML with the same token classes', () => {
    const html = highlightCodeFenceHtml('if True:  # ok\n')
    expect(html).toContain('<span class="token-keyword">if</span>')
    expect(html).toContain('<span class="token-keyword">True</span>')
    expect(html).toContain('<span class="token-comment"># ok</span>')
    expect(html.endsWith('\n')).toBe(true)
  })

  it('escapes raw HTML in highlighted fences', () => {
    expect(highlightCodeFenceHtml('x < y & "z"\n')).toBe(
      'x &lt; y &amp; <span class="token-string">&quot;z&quot;</span>\n'
    )
  })
})

describe('workspace file preview find', () => {
  it('finds case-insensitive occurrences across lines', () => {
    expect(findFilePreviewMatches('Alpha\nbeta ALPHA\n', 'alpha')).toEqual([
      { lineIndex: 0, start: 0, end: 5 },
      { lineIndex: 1, start: 5, end: 10 }
    ])
  })

  it('returns no matches for blank queries', () => {
    expect(findFilePreviewMatches('hello', '   ')).toEqual([])
  })

  it('splits a line into plain and match parts', () => {
    const matches = findFilePreviewMatches('foo bar foo', 'foo')
    expect(filePreviewSearchParts('foo bar foo', 0, matches)).toEqual([
      { text: 'foo', matchIndex: 0 },
      { text: ' bar ' },
      { text: 'foo', matchIndex: 1 }
    ])
  })
})
