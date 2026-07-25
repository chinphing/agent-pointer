import { describe, expect, it } from 'vitest'
import { findDiffChangeBlocks, findDiffMatches, showDiffWhitespace } from './diffView'
import type { DiffLine } from '../components/chat/DiffView.vue'

const lines: DiffLine[] = [
  { type: 'unchanged', text: 'before' },
  { type: 'del', text: 'old value' },
  { type: 'ins', text: 'new value' },
  { type: 'unchanged', text: 'middle' },
  { type: 'collapse', text: '2', hidden: ['hidden target', 'other'] },
  { type: 'ins', text: 'target visible' }
]

describe('diff view helpers', () => {
  it('finds visible and collapsed matches case-insensitively', () => {
    expect(findDiffMatches(lines, 'TARGET')).toEqual([
      { key: 'hidden-4-0', parentIndex: 4 },
      { key: 'line-5' }
    ])
    expect(findDiffMatches(lines, '  ')).toEqual([])
  })

  it('groups adjacent inserted and deleted rows into change blocks', () => {
    expect(findDiffChangeBlocks(lines)).toEqual(['line-1', 'line-5'])
  })

  it('renders spaces and tabs without removing the tab width', () => {
    expect(showDiffWhitespace(' a\tb ')).toBe('·a→\tb·')
  })
})
