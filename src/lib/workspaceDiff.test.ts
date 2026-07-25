import { describe, expect, it } from 'vitest'
import { parseWorkspaceDiff } from './workspaceDiff'

describe('parseWorkspaceDiff', () => {
  it('parses Git hunk line numbers and stats', () => {
    const result = parseWorkspaceDiff([
      'diff --git a/a.ts b/a.ts',
      'index 123..456 100644',
      '--- a/a.ts',
      '+++ b/a.ts',
      '@@ -42,2 +48,2 @@',
      ' const a = 1',
      '-const b = 2',
      '+const b = 3',
      '\\ No newline at end of file'
    ].join('\n'))

    expect(result.stats).toEqual({ adds: 1, dels: 1 })
    expect(result.lines).toEqual([
      { type: 'collapse', text: '@@ -42,2 +48,2 @@', hidden: [], isHunkHeader: true },
      { type: 'unchanged', text: 'const a = 1', oldLine: 42, newLine: 48 },
      { type: 'del', text: 'const b = 2', oldLine: 43 },
      { type: 'ins', text: 'const b = 3', newLine: 49 },
      { type: 'unchanged', text: '\\ No newline at end of file', oldLine: 43, newLine: 49 }
    ])
  })

  it('collapses long unchanged runs while preserving surrounding Git line numbers', () => {
    const context = Array.from({ length: 10 }, (_, index) => ` line ${index + 10}`)
    const result = parseWorkspaceDiff([
      '@@ -10,12 +20,12 @@',
      ...context,
      '-removed',
      '+added'
    ].join('\n'))

    expect(result.lines).toEqual([
      { type: 'collapse', text: '@@ -10,12 +20,12 @@', hidden: [], isHunkHeader: true },
      { type: 'unchanged', text: 'line 10', oldLine: 10, newLine: 20 },
      { type: 'unchanged', text: 'line 11', oldLine: 11, newLine: 21 },
      { type: 'unchanged', text: 'line 12', oldLine: 12, newLine: 22 },
      {
        type: 'collapse',
        text: '4',
        hidden: ['line 13', 'line 14', 'line 15', 'line 16'],
        hiddenOldLine: 13,
        hiddenNewLine: 23
      },
      { type: 'unchanged', text: 'line 17', oldLine: 17, newLine: 27 },
      { type: 'unchanged', text: 'line 18', oldLine: 18, newLine: 28 },
      { type: 'unchanged', text: 'line 19', oldLine: 19, newLine: 29 },
      { type: 'del', text: 'removed', oldLine: 20 },
      { type: 'ins', text: 'added', newLine: 30 }
    ])
  })

  it('keeps separate hunk headers and resets line counters for each hunk', () => {
    const result = parseWorkspaceDiff([
      '@@ -2 +5 @@ first',
      '-old',
      '+new',
      '@@ -100,2 +200,2 @@ second',
      ' unchanged',
      '-old again',
      '+new again'
    ].join('\n'))

    expect(result.lines).toEqual([
      { type: 'collapse', text: '@@ -2 +5 @@ first', hidden: [], isHunkHeader: true },
      { type: 'del', text: 'old', oldLine: 2 },
      { type: 'ins', text: 'new', newLine: 5 },
      { type: 'collapse', text: '@@ -100,2 +200,2 @@ second', hidden: [], isHunkHeader: true },
      { type: 'unchanged', text: 'unchanged', oldLine: 100, newLine: 200 },
      { type: 'del', text: 'old again', oldLine: 101 },
      { type: 'ins', text: 'new again', newLine: 201 }
    ])
  })

  it('returns an empty model for metadata-only diffs', () => {
    expect(parseWorkspaceDiff('diff --git a/x b/x\nindex 1..2 100644\n').lines).toEqual([])
  })
})
