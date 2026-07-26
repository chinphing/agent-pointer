import { describe, expect, it } from 'vitest'
import { computeDiffLines, fileEditSnippetFromArgs } from './textDiff'

describe('computeDiffLines', () => {
  it('reports insert and delete stats', () => {
    const { diffLines, diffStats } = computeDiffLines('a\nb\n', 'a\nc\n')
    expect(diffStats).toEqual({ adds: 1, dels: 1 })
    expect(diffLines.some(line => line.type === 'del' && line.text === 'b')).toBe(true)
    expect(diffLines.some(line => line.type === 'ins' && line.text === 'c')).toBe(true)
  })

  it('folds long unchanged runs', () => {
    const old = Array.from({ length: 20 }, (_, i) => `L${i}`).join('\n')
    const next = Array.from({ length: 20 }, (_, i) => (i === 10 ? 'CHANGED' : `L${i}`)).join('\n')
    const { diffLines, diffStats } = computeDiffLines(old, next)
    expect(diffStats).toEqual({ adds: 1, dels: 1 })
    expect(diffLines.some(line => line.type === 'collapse')).toBe(true)
  })
})

describe('fileEditSnippetFromArgs', () => {
  it('reads camelCase and snake_case keys', () => {
    expect(
      fileEditSnippetFromArgs(JSON.stringify({ oldString: 'a', newString: 'b' }))
    ).toEqual({ oldString: 'a', newString: 'b' })
    expect(
      fileEditSnippetFromArgs(JSON.stringify({ old_string: 'a', new_string: 'b' }))
    ).toEqual({ oldString: 'a', newString: 'b' })
  })
})
