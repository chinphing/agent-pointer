import { describe, expect, it } from 'vitest'
import {
  filterPreviewTabsForConversation,
  workspaceActiveAfterClose,
  workspacePreviewTabId,
  workspaceTabIdsToClose
} from './workspaceTabs'

describe('workspace preview tabs', () => {
  const ids = ['file:a.ts', 'diff:a.ts', 'file:b.ts']

  it('deduplicates identity by kind and path', () => {
    expect(workspacePreviewTabId('file', 'a.ts')).toBe('file:a.ts')
    expect(workspacePreviewTabId('diff', 'a.ts')).toBe('diff:a.ts')
    expect(workspacePreviewTabId('turn-diff', 'a.ts', 'turn-1')).toBe('turn-diff:turn-1:a.ts')
  })

  it('selects close scopes', () => {
    expect(workspaceTabIdsToClose(ids, 'diff:a.ts', 'close')).toEqual(['diff:a.ts'])
    expect(workspaceTabIdsToClose(ids, 'diff:a.ts', 'close-others')).toEqual(['file:a.ts', 'file:b.ts'])
    expect(workspaceTabIdsToClose(ids, 'diff:a.ts', 'close-right')).toEqual(['file:b.ts'])
  })

  it('keeps active tabs or picks right then left after closing', () => {
    expect(workspaceActiveAfterClose(ids, 'file:a.ts', ['file:b.ts'], 'file:b.ts', 'files')).toBe('file:a.ts')
    expect(workspaceActiveAfterClose(ids, 'diff:a.ts', ['diff:a.ts'], 'diff:a.ts', 'changes')).toBe('file:b.ts')
    expect(workspaceActiveAfterClose(ids, 'file:b.ts', ['file:b.ts'], 'file:b.ts', 'files')).toBe('diff:a.ts')
    expect(workspaceActiveAfterClose(['file:a.ts'], 'file:a.ts', ['file:a.ts'], 'file:a.ts', 'files')).toBe('files')
  })

  it('keeps file/git tabs but drops other conversations turn-diffs', () => {
    const tabs = [
      { id: 'file:a.ts', kind: 'file' },
      { id: 'diff:a.ts', kind: 'diff' },
      { id: 'turn-diff:t1:a.ts', kind: 'turn-diff', conversationId: 'c1' },
      { id: 'turn-diff:t2:b.ts', kind: 'turn-diff', conversationId: 'c2' }
    ]
    expect(filterPreviewTabsForConversation(tabs, 'c2').map(tab => tab.id)).toEqual([
      'file:a.ts',
      'diff:a.ts',
      'turn-diff:t2:b.ts'
    ])
  })
})
