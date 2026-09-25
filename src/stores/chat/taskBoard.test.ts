import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../../types/chat'
import {
  applyTaskBoardDocumentToEntry,
  childBoardBindingForTrace,
  childStoreKey,
  childStoreKeyForInstance,
  emptyTaskBoardEntry,
  resolveActiveParentBoardBinding,
  resolveActiveParentBoardDocument,
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
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('click button'), 'task_a:computer', [])
    const out = resolveCompactTaskBoardDocument(entry, 'msg_lead', 'task_a', 'task_a:computer')
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

  it('keeps parent binding when a later update carries a newer explicit anchor', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = `conv1${TASK_BOARD_MAIN_TURN_SEP}u_original`
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('goal'), 'u_original', [])
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      storeKey,
      doc('goal updated'),
      'u_continue',
      [
        { id: 'u_original', role: 'user', content: 'start', status: 'done', createdAt: 0 },
        { id: 'u_continue', role: 'user', content: '继续', status: 'done', createdAt: 1 }
      ]
    )
    expect(entry.parentBindings[storeKey]).toBe('u_original')
  })

  it('preserves parent binding on update without explicit anchor', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = `conv1${TASK_BOARD_MAIN_TURN_SEP}u_original`
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('goal'), 'u_original', [])
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      storeKey,
      doc('goal updated'),
      undefined,
      [
        { id: 'u_original', role: 'user', content: 'start', status: 'done', createdAt: 0 },
        { id: 'u_continue', role: 'user', content: '继续', status: 'done', createdAt: 1 }
      ]
    )
    expect(entry.parentBindings[storeKey]).toBe('u_original')
  })

  it('legacy conv id store key does not rebind to latest user on update', () => {
    const entry = emptyTaskBoardEntry()
    applyTaskBoardDocumentToEntry(entry, 'conv1', 'conv1', doc('legacy'), 'u_original', [])
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      'conv1',
      doc('legacy updated'),
      undefined,
      [
        { id: 'u_original', role: 'user', content: 'start', status: 'done', createdAt: 0 },
        { id: 'u_continue', role: 'user', content: '继续', status: 'done', createdAt: 1 }
      ]
    )
    expect(entry.parentBindings.conv1).toBe('u_original')
  })

  it('binds new main-turn store key from embedded user message id', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = `conv1${TASK_BOARD_MAIN_TURN_SEP}u_new`
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      storeKey,
      doc('new task'),
      undefined,
      [
        { id: 'u_original', role: 'user', content: 'old', status: 'done', createdAt: 0 },
        { id: 'u_new', role: 'user', content: 'new task', status: 'done', createdAt: 1 }
      ]
    )
    expect(entry.parentBindings[storeKey]).toBe('u_new')
  })

  it('resolveChildTaskBoardDocument respects trace id anchor', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('child'), 'task_a:computer', [])
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_a:computer')?.meta?.goal).toBe('child')
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_b:computer')).toBeNull()
  })

  it('resolveChildTaskBoardDocument falls back to legacy lead message anchor', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('child'), 'lead_assistant', [])
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_a:computer', 'lead_assistant')?.meta?.goal).toBe('child')
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_a:computer', 'other_msg')).toBeNull()
  })

  it('resolveChildTaskBoardDocument hides child board bound to scoped id from trace lookup', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('child'), 'scoped_round', [])
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_a:computer', 'lead_assistant')).toBeNull()
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('child'), 'task_a:computer', [])
    expect(resolveChildTaskBoardDocument(entry, 'task_a', 'task_a:computer')?.meta?.goal).toBe('child')
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
      'task_a:computer',
      []
    )
    expect(storeKey).toContain(TASK_BOARD_SUB_SEP)
    expect(entry.childrenByParentStoreKey[parentKey]?.[taskId]?.meta?.goal).toBe('sub task')
    expect(entry.childBindings[storeKey]).toBe('task_a:computer')
  })

  it('childBoardBindingForTrace returns child board for SubAgentFrame', () => {
    const entry = emptyTaskBoardEntry()
    const storeKey = childStoreKey('conv1', 'task_a')
    applyTaskBoardDocumentToEntry(entry, 'conv1', storeKey, doc('open wechat', 'active'), 'task_a:computer', [])
    const binding = childBoardBindingForTrace(entry, 'task_a:computer', 'lead_msg')
    expect(binding?.storeKey).toBe(storeKey)
    expect(binding?.document.meta?.goal).toBe('open wechat')
    expect(binding?.isActive).toBe(true)
    expect(childBoardBindingForTrace(entry, 'task_b:computer', 'lead_msg')).toBeNull()
  })

  it('binds coder self-fork child board keyed with agent instance id', () => {
    const entry = emptyTaskBoardEntry()
    const taskId = 'call_3OprsqC3ip30GcwFq6f7mEWZ'
    const instanceId = '9f296493-7477-4608-b488-7865ad1c9e70'
    const traceId = `${taskId}:${instanceId}:coder`
    const storeKey = childStoreKeyForInstance('conv1', taskId, instanceId)
    applyTaskBoardDocumentToEntry(
      entry,
      'conv1',
      storeKey,
      doc('fullscreen settings', 'completed'),
      traceId,
      []
    )
    const binding = childBoardBindingForTrace(entry, traceId, 'lead_assistant')
    expect(binding?.storeKey).toBe(storeKey)
    expect(binding?.document.meta?.goal).toBe('fullscreen settings')
    expect(binding?.isActive).toBe(false)
    expect(resolveChildTaskBoardDocument(entry, taskId, traceId)?.meta?.goal).toBe(
      'fullscreen settings'
    )
  })

  it('picks the matching instance when two self-fork boards share a task id', () => {
    const entry = emptyTaskBoardEntry()
    const taskId = 'shared-task'
    const a = childStoreKeyForInstance('conv1', taskId, 'instance-a')
    const b = childStoreKeyForInstance('conv1', taskId, 'instance-b')
    applyTaskBoardDocumentToEntry(entry, 'conv1', a, doc('fork a'), `${taskId}:instance-a:coder`, [])
    applyTaskBoardDocumentToEntry(entry, 'conv1', b, doc('fork b'), `${taskId}:instance-b:coder`, [])
    expect(
      childBoardBindingForTrace(entry, `${taskId}:instance-b:coder`)?.document.meta?.goal
    ).toBe('fork b')
    expect(
      childBoardBindingForTrace(entry, `${taskId}:instance-a:coder`)?.document.meta?.goal
    ).toBe('fork a')
  })
})
