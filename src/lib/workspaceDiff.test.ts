import { describe, expect, it } from 'vitest'
import { parseWorkspaceDiff } from './workspaceDiff'

describe('parseWorkspaceDiff', () => {
  it('parses unified diff content and stats', () => {
    const result = parseWorkspaceDiff([
      'diff --git a/a.ts b/a.ts',
      'index 123..456 100644',
      '--- a/a.ts',
      '+++ b/a.ts',
      '@@ -1,2 +1,2 @@',
      ' const a = 1',
      '-const b = 2',
      '+const b = 3',
      '\\ No newline at end of file'
    ].join('\n'))

    expect(result.stats).toEqual({ adds: 1, dels: 1 })
    expect(result.lines).toEqual([
      { type: 'collapse', text: '@@ -1,2 +1,2 @@', hidden: [] },
      { type: 'unchanged', text: 'const a = 1' },
      { type: 'del', text: 'const b = 2' },
      { type: 'ins', text: 'const b = 3' },
      { type: 'unchanged', text: '\\ No newline at end of file' }
    ])
  })

  it('returns an empty model for metadata-only diffs', () => {
    expect(parseWorkspaceDiff('diff --git a/x b/x\nindex 1..2 100644\n').lines).toEqual([])
  })
})
