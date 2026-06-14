import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../../types/chat'
import {
  applyTaskBoardDocumentToEntry,
  childStoreKey,
  emptyTaskBoardEntry,
  resolveActiveParentBoardDocument,
  resolveActiveParentBoardBinding,
  resolveChildTaskBoardDocument,
  resolveCompactTaskBoardDocument,
  TASK_BOARD_MAIN_TURN_SEP,
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

  it('resolveActiveParentBoardDocument falls back to active parent when anchor is user id', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('open browser'), 'u1', [])
    expect(resolveActiveParentBoardDocument(entry, 'assistant_msg')).toEqual(entry.parentByStoreKey.conv1)
    expect(resolveActiveParentBoardDocument(entry, 'u1')).toEqual(entry.parentByStoreKey.conv1)
  })

  it('resolveCompactTaskBoardDocument prefers child board for delegated computer sub-agent', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('supervisor goal'), 'u1', [])
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('click button'), 'msg_lead', [])
    const out = resolveCompactTaskBoardDocument(entry, 'msg_lead', 'task_a')
    expect(out?.meta?.goal).toBe('click button')
  })

  it('resolveCompactTaskBoardDocument uses parent board for lead computer without sub task id', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('solo computer'), 'u1', [])
    const out = resolveCompactTaskBoardDocument(entry, 'assistant_msg', null)
    expect(out?.meta?.goal).toBe('solo computer')
  })

  it('resolveCompactTaskBoardDocument reuses active parent after resume on new assistant id', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = `conv1${TASK_BOARD_MAIN_TURN_SEP}u_original`
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('open wechat'), 'u_original', [])
    const out = resolveCompactTaskBoardDocument(entry, 'assistant_after_continue', null)
    expect(out?.meta?.goal).toBe('open wechat')
  })

  it('resolveActiveParentBoardBinding returns store key for compact resume', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = `conv1${TASK_BOARD_MAIN_TURN_SEP}u_original`
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('resume goal'), 'u_original', [])
    const binding = resolveActiveParentBoardBinding(entry, 'u_continue')
    expect(binding?.storeKey).toBe(storeKey)
    expect(binding?.document.meta?.goal).toBe('resume goal')
    expect(binding?.isActive).toBe(true)
  })

  it('resolveChildTaskBoardDocument respects message anchor', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('child'), 'msg_a', [])
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'msg_a')?.meta?.goal).toBe('child')
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'msg_other')).toBeNull()
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
