import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../../types/chat'
import {
  applyTaskBoardDocumentToEntry,
  childStoreKey,
  emptyTaskBoardEntry,
  TASK_BOARD_SUB_SEP
} from './taskBoard'

function doc(goal: string, status = 'active'): TaskBoardDocument {
  return {
    version: 2,
    task_id: 'tb_conv1',
    meta: { goal, status },
    board: []
  }
}

describe('taskBoard logic', () => {
  it('stores parent board and binds anchor from fallback message', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      'conv1',
      doc('build feature'),
      undefined,
      [{ id: 'u1', role: 'user', content: 'go', status: 'done', createdAt: 0 }]
    )
    expect(entry.parentByStoreKey.conv1?.meta?.goal).toBe('build feature')
    expect(entry.parentBindings.conv1).toBe('u1')
    expect(entry.activeParentStoreKey).toBe('conv1')
  })

  it('clears active parent when board reaches terminal status', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('x', 'active'), 'u1', [])
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('x', 'completed'), 'u1', [])
    expect(entry.activeParentStoreKey).toBeNull()
  })

  it('stores child board under parent store key', () => {
    const entry = emptyTaskBoardEntry()
    const parentKey = 'conv1'
    const taskId = 'task_a'
    const storeKey = childStoreKey(parentKey, taskId)
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      storeKey,
      doc('sub task'),
      'msg_sub',
      []
    )
    expect(storeKey).toContain(TASK_BOARD_SUB_SEP)
    expect(entry.childrenByParentStoreKey[parentKey]?.[taskId]?.meta?.goal).toBe('sub task')
    expect(entry.childBindings[storeKey]).toBe('msg_sub')
  })
})
