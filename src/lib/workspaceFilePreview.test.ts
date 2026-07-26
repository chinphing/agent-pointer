import { describe, expect, it } from 'vitest'
import {
  filePreviewSearchParts,
  findFilePreviewMatches,
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
