import { describe, expect, it } from 'vitest'
import { tokenizeCodeLine } from './workspaceFilePreview'

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
