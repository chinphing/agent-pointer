import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import {
  collectLeadTurnStarts,
  fileChangesByTurn,
  frozenFileChangesFromStarts,
  lastTurnFileChanges,
  resolveActiveTurnFileChanges
} from './lastTurnFileChanges'

function tc(partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name'>): ToolCall {
  return {
    status: 'success',
    arguments: '{}',
    ...partial
  }
}

function msg(partial: Partial<ChatMessage> & Pick<ChatMessage, 'id' | 'role'>): ChatMessage {
  return {
    content: '',
    status: 'done',
    createdAt: 0,
    ...partial
  }
}

describe('lastTurnFileChanges', () => {
  it('returns only files from the latest user turn', () => {
    const result = lastTurnFileChanges([
      msg({
        id: 'u1',
        role: 'user',
        content: 'first',
        toolCalls: [
          tc({
            id: 'old',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/old.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      }),
      msg({ id: 'a1', role: 'assistant', content: 'done' }),
      msg({ id: 'u2', role: 'user', content: 'second' }),
      msg({
        id: 'a2',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'w1',
            name: 'file_write',
            result: JSON.stringify({ path: '/ws/new.ts', success: true, bytesWritten: 3 })
          }),
          tc({
            id: 'e1',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/edit.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 1 }
            })
          })
        ]
      })
    ])

    expect(result?.turnId).toBe('u2')
    // Sorted by basename: edit.ts, then new.ts (not tool-call order).
    expect(result?.files.map(f => f.path)).toEqual(['/ws/edit.ts', '/ws/new.ts'])
  })

  it('sorts files by basename, not first-edit time', () => {
    const result = lastTurnFileChanges([
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({
        id: 'a1',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'z',
            name: 'file_write',
            result: JSON.stringify({ path: '/ws/z.ts', success: true, bytesWritten: 1 })
          }),
          tc({
            id: 'a',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/a.ts',
              success: true,
              replaced: 1,
              stats: { adds: 2, dels: 1 }
            })
          })
        ]
      })
    ])
    expect(result?.files.map(f => f.fileName)).toEqual(['a.ts', 'z.ts'])
  })

  it('returns null when latest turn has no file mutations', () => {
    expect(
      lastTurnFileChanges([
        msg({ id: 'u1', role: 'user', content: 'hi' }),
        msg({ id: 'a1', role: 'assistant', content: 'hello' })
      ])
    ).toBeNull()
  })

  it('anchors to the lead user turn when a scoped sub-agent stub follows', () => {
    const result = lastTurnFileChanges([
      msg({ id: 'u1', role: 'user', content: 'edit via coder' }),
      msg({ id: 'a1', role: 'assistant', content: '' }),
      msg({
        id: 'sub_stub',
        role: 'user',
        content: 'Begin. Your assigned task is in the system prompt under **Assigned task**.',
        anchorMessageId: 'a1'
      }),
      msg({
        id: 'sub_asst',
        role: 'assistant',
        content: '',
        anchorMessageId: 'a1',
        toolCalls: [
          tc({
            id: 'e1',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/from-sub.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      })
    ])

    expect(result?.turnId).toBe('u1')
    expect(result?.files.map(f => f.path)).toEqual(['/ws/from-sub.ts'])
  })

  it('maps successful writes onto each lead turn without merging later turns', () => {
    const byTurn = fileChangesByTurn([
      msg({
        id: 'u1',
        role: 'user',
        content: 'first',
        toolCalls: [
          tc({
            id: 'old',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/old.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      }),
      msg({ id: 'a1', role: 'assistant', content: 'done' }),
      msg({ id: 'u2', role: 'user', content: 'second' }),
      msg({
        id: 'a2',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'w1',
            name: 'file_write',
            result: JSON.stringify({ path: '/ws/new.ts', success: true, bytesWritten: 3 })
          })
        ]
      })
    ])

    expect([...byTurn.keys()]).toEqual(['u1', 'u2'])
    expect(byTurn.get('u1')?.map(f => f.path)).toEqual(['/ws/old.ts'])
    expect(byTurn.get('u2')?.map(f => f.path)).toEqual(['/ws/new.ts'])
  })

  it('lastTurnFileChanges only needs the latest lead-turn slice', () => {
    const result = lastTurnFileChanges([
      msg({
        id: 'u1',
        role: 'user',
        content: 'old',
        toolCalls: [
          tc({
            id: 'old',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/old.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      }),
      msg({ id: 'u2', role: 'user', content: 'now' }),
      msg({
        id: 'a2',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({ id: 'g1', name: 'file_grep', result: JSON.stringify({ matches: 3 }) })
        ]
      })
    ])
    expect(result).toBeNull()
  })

  it('resolveActiveTurnFileChanges waits until the tool batch settles', () => {
    const edit = tc({
      id: 'e1',
      name: 'file_edit',
      status: 'running',
      result: ''
    })
    const grep = tc({
      id: 'g1',
      name: 'file_grep',
      status: 'running',
      result: ''
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({ id: 'a1', role: 'assistant', content: '', toolCalls: [edit, grep] })
    ]
    const inflight = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    expect(inflight.files).toEqual([])

    edit.status = 'success'
    edit.result = JSON.stringify({
      path: '/ws/a.ts',
      success: true,
      replaced: 1,
      stats: { adds: 2, dels: 1 }
    })
    const stillWaiting = resolveActiveTurnFileChanges(list, 'u1', 0, inflight)
    expect(stillWaiting).toBe(inflight)
    expect(stillWaiting.files).toEqual([])

    grep.status = 'success'
    grep.result = JSON.stringify({ matches: 3 })
    const settled = resolveActiveTurnFileChanges(list, 'u1', 0, stillWaiting)
    expect(settled).not.toBe(stillWaiting)
    expect(settled.files.map(f => f.path)).toEqual(['/ws/a.ts'])
  })

  it('resolveActiveTurnFileChanges keeps the same files array after a grep-only batch', () => {
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({
        id: 'a1',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'e1',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/a.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      })
    ]
    const first = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    expect(first.files.map(f => f.path)).toEqual(['/ws/a.ts'])

    const grep = tc({
      id: 'g1',
      name: 'file_grep',
      status: 'running',
      result: ''
    })
    list.push(msg({ id: 'a2', role: 'assistant', content: '', toolCalls: [grep] }))
    const waiting = resolveActiveTurnFileChanges(list, 'u1', 0, first)
    expect(waiting).toBe(first)

    grep.status = 'success'
    grep.result = JSON.stringify({ matches: 9 })
    const afterGrep = resolveActiveTurnFileChanges(list, 'u1', 0, waiting)
    expect(afterGrep).toBe(first)
    expect(afterGrep.files).toBe(first.files)
  })

  it('resolveActiveTurnFileChanges updates when one message batch settles while another is still running', () => {
    const write = tc({
      id: 'w1',
      name: 'file_write',
      status: 'running',
      result: ''
    })
    const grep = tc({
      id: 'g1',
      name: 'file_grep',
      status: 'running',
      result: ''
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({ id: 'coder', role: 'assistant', content: '', toolCalls: [write] }),
      msg({ id: 'explore', role: 'assistant', content: '', toolCalls: [grep] })
    ]
    const inflight = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    expect(inflight.files).toEqual([])

    write.status = 'success'
    write.result = JSON.stringify({ path: '/ws/a.ts', success: true, bytesWritten: 3 })
    const afterWrite = resolveActiveTurnFileChanges(list, 'u1', 0, inflight)
    expect(afterWrite.files.map(f => f.path)).toEqual(['/ws/a.ts'])
    expect(grep.status).toBe('running')
  })

  it('resolveActiveTurnFileChanges merges a later batch without rescanning earlier writes', () => {
    const firstEdit = tc({
      id: 'e1',
      name: 'file_edit',
      result: JSON.stringify({
        path: '/ws/a.ts',
        success: true,
        replaced: 1,
        stats: { adds: 2, dels: 1 }
      })
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({ id: 'a1', role: 'assistant', content: '', toolCalls: [firstEdit] })
    ]
    const first = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    const firstA = first.files.find(f => f.path === '/ws/a.ts')
    expect(firstA?.adds).toBe(2)

    const secondWrite = tc({
      id: 'w1',
      name: 'file_write',
      status: 'running',
      result: ''
    })
    list.push(msg({ id: 'a2', role: 'assistant', content: '', toolCalls: [secondWrite] }))
    const waiting = resolveActiveTurnFileChanges(list, 'u1', 0, first)
    expect(waiting).toBe(first)

    secondWrite.status = 'success'
    secondWrite.result = JSON.stringify({ path: '/ws/b.ts', success: true, bytesWritten: 3 })
    const merged = resolveActiveTurnFileChanges(list, 'u1', 0, waiting)
    expect(merged).not.toBe(first)
    expect(merged.files.map(f => f.path)).toEqual(['/ws/a.ts', '/ws/b.ts'])
    expect(merged.files.find(f => f.path === '/ws/a.ts')).toBe(firstA)
  })

  it('resolveActiveTurnFileChanges accumulates stats when the same path is edited again', () => {
    const firstEdit = tc({
      id: 'e1',
      name: 'file_edit',
      result: JSON.stringify({
        path: '/ws/a.ts',
        success: true,
        replaced: 1,
        stats: { adds: 2, dels: 1 }
      })
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({ id: 'a1', role: 'assistant', content: '', toolCalls: [firstEdit] })
    ]
    const first = resolveActiveTurnFileChanges(list, 'u1', 0, null)

    const secondEdit = tc({
      id: 'e2',
      name: 'file_edit',
      status: 'running',
      result: ''
    })
    list.push(msg({ id: 'a2', role: 'assistant', content: '', toolCalls: [secondEdit] }))
    resolveActiveTurnFileChanges(list, 'u1', 0, first)
    secondEdit.status = 'success'
    secondEdit.result = JSON.stringify({
      path: '/ws/a.ts',
      success: true,
      replaced: 1,
      stats: { adds: 3, dels: 0 }
    })
    const merged = resolveActiveTurnFileChanges(list, 'u1', 0, first)
    expect(merged.files).toHaveLength(1)
    expect(merged.files[0]?.adds).toBe(5)
    expect(merged.files[0]?.dels).toBe(1)
    expect(merged.files[0]).not.toBe(first.files[0])
  })

  it('resolveActiveTurnFileChanges merges two messages that settle in the same tick', () => {
    const writeA = tc({
      id: 'w1',
      name: 'file_write',
      status: 'running',
      result: ''
    })
    const writeB = tc({
      id: 'w2',
      name: 'file_write',
      status: 'running',
      result: ''
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({ id: 'coder', role: 'assistant', content: '', toolCalls: [writeA] }),
      msg({ id: 'explore', role: 'assistant', content: '', toolCalls: [writeB] })
    ]
    const inflight = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    writeA.status = 'success'
    writeA.result = JSON.stringify({ path: '/ws/a.ts', success: true, bytesWritten: 1 })
    writeB.status = 'success'
    writeB.result = JSON.stringify({ path: '/ws/b.ts', success: true, bytesWritten: 1 })
    const settled = resolveActiveTurnFileChanges(list, 'u1', 0, inflight)
    expect(settled.files.map(f => f.path)).toEqual(['/ws/a.ts', '/ws/b.ts'])
  })

  it('collectLeadTurnStarts reuses the same list when only length-stable', () => {
    const list = [
      msg({ id: 'u1', role: 'user', content: 'a' }),
      msg({ id: 'a1', role: 'assistant', content: 'b' })
    ]
    const first = collectLeadTurnStarts(list)
    expect(collectLeadTurnStarts(list)).toBe(first)
    list.push(msg({ id: 'a2', role: 'assistant', content: 'c' }))
    const afterPush = collectLeadTurnStarts(list)
    expect(afterPush).toBe(first)
    expect(afterPush.map(item => item.turnId)).toEqual(['u1'])
    list.push(msg({ id: 'u2', role: 'user', content: 'd' }))
    expect(collectLeadTurnStarts(list).map(item => item.turnId)).toEqual(['u1', 'u2'])
  })

  it('frozenFileChangesFromStarts keeps older file arrays when a new lead turn starts', () => {
    const list = [
      msg({
        id: 'u1',
        role: 'user',
        content: 'first',
        toolCalls: [
          tc({
            id: 'old',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/old.ts',
              success: true,
              replaced: 1,
              stats: { adds: 1, dels: 0 }
            })
          })
        ]
      }),
      msg({ id: 'a1', role: 'assistant', content: 'done' }),
      msg({ id: 'u2', role: 'user', content: 'second' }),
      msg({
        id: 'a2',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'w1',
            name: 'file_write',
            result: JSON.stringify({ path: '/ws/new.ts', success: true, bytesWritten: 3 })
          })
        ]
      })
    ]
    const firstStarts = collectLeadTurnStarts(list)
    const first = frozenFileChangesFromStarts(list, firstStarts, null)
    expect(first.map.get('u1')?.map(f => f.path)).toEqual(['/ws/old.ts'])
    expect(frozenFileChangesFromStarts(list, firstStarts, first)).toBe(first)

    list.push(msg({ id: 'u3', role: 'user', content: 'third' }))
    const secondStarts = collectLeadTurnStarts(list)
    const second = frozenFileChangesFromStarts(list, secondStarts, first)
    expect(second).not.toBe(first)
    expect(second.map.get('u1')).toBe(first.map.get('u1'))
    expect(second.map.get('u2')?.map(f => f.path)).toEqual(['/ws/new.ts'])
  })
})
