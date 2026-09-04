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

  it('keeps edits after an empty-reply retry inject on the original user turn', () => {
    const result = lastTurnFileChanges([
      msg({ id: 'u1', role: 'user', content: 'edit files' }),
      msg({ id: 'a1', role: 'assistant', content: '' }),
      msg({
        id: 'fmt_retry_1',
        role: 'user',
        content: '你的上一次回复为空，请重新输出。'
      }),
      msg({
        id: 'a2',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'e1',
            name: 'file_edit',
            result: JSON.stringify({
              path: '/ws/Composer.vue',
              success: true,
              replaced: 1,
              stats: { adds: 2, dels: 0 }
            })
          })
        ]
      })
    ])
    expect(result?.turnId).toBe('u1')
    expect(result?.files.map(f => f.path)).toEqual(['/ws/Composer.vue'])
  })

  it('includes file_edit on agentTrace.session while the parent run_subagent is still running', () => {
    const result = lastTurnFileChanges([
      msg({ id: 'u1', role: 'user', content: 'edit via m4' }),
      msg({
        id: 'a1',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'run1',
            name: 'run_subagent',
            status: 'running',
            arguments: JSON.stringify({ agent: 'explore' })
          })
        ],
        agentTrace: [
          {
            id: 'task_a:explore',
            name: 'm4',
            role: 'explore',
            status: 'running',
            session: {
              collapsed: true,
              userExpanded: false,
              stats: { searchCount: 2, readCount: 1 },
              toolCalls: [
                tc({
                  id: 'e1',
                  name: 'file_edit',
                  result: JSON.stringify({
                    path: '/ws/AttachmentChip.vue',
                    success: true,
                    replaced: 1,
                    stats: { adds: 4, dels: 1 }
                  })
                })
              ]
            }
          }
        ]
      })
    ])
    expect(result?.turnId).toBe('u1')
    expect(result?.files.map(f => f.path)).toEqual(['/ws/AttachmentChip.vue'])
  })

  it('resolveActiveTurnFileChanges settles a nested session without waiting for run_subagent', () => {
    const nestedEdit = tc({
      id: 'e1',
      name: 'file_edit',
      status: 'running',
      result: ''
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'go' }),
      msg({
        id: 'a1',
        role: 'assistant',
        content: '',
        toolCalls: [
          tc({
            id: 'run1',
            name: 'run_subagent',
            status: 'running',
            arguments: '{}'
          })
        ],
        agentTrace: [
          {
            id: 'task_a:explore',
            name: 'm4',
            role: 'explore',
            status: 'running',
            session: {
              collapsed: true,
              userExpanded: false,
              stats: { searchCount: 0, readCount: 0 },
              toolCalls: [nestedEdit]
            }
          }
        ]
      })
    ]
    const inflight = resolveActiveTurnFileChanges(list, 'u1', 0, null)
    expect(inflight.files).toEqual([])

    nestedEdit.status = 'success'
    nestedEdit.result = JSON.stringify({
      path: '/ws/Composer.vue',
      success: true,
      replaced: 1,
      stats: { adds: 2, dels: 0 }
    })
    const settled = resolveActiveTurnFileChanges(list, 'u1', 0, inflight)
    expect(settled.files.map(f => f.path)).toEqual(['/ws/Composer.vue'])
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

  it('resolveActiveTurnFileChanges merges a successful edit while later tools on the same message are still running', () => {
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
    const afterEdit = resolveActiveTurnFileChanges(list, 'u1', 0, inflight)
    expect(afterEdit.files.map(f => f.path)).toEqual(['/ws/a.ts'])
    expect(grep.status).toBe('running')

    grep.status = 'success'
    grep.result = JSON.stringify({ matches: 3 })
    const afterGrep = resolveActiveTurnFileChanges(list, 'u1', 0, afterEdit)
    expect(afterGrep).toBe(afterEdit)
    expect(afterGrep.files).toBe(afterEdit.files)
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

  it('does not split file-change totals across an in-run compression summary', () => {
    const editBefore = tc({
      id: 'e1',
      name: 'file_edit',
      result: JSON.stringify({
        path: '/ws/a.ts',
        success: true,
        replaced: 1,
        stats: { adds: 2, dels: 1 }
      })
    })
    const writeAfter = tc({
      id: 'w1',
      name: 'file_write',
      result: JSON.stringify({ path: '/ws/b.ts', success: true, bytesWritten: 8 })
    })
    const summaries: ChatMessage[] = [
      msg({
        id: 'ctx_user',
        role: 'user',
        content: '[Conversation summary (auto-compression)]\nmid'
      }),
      msg({
        id: 'ctx_asst',
        role: 'assistant',
        content: '[Conversation summary (auto-compression)]\nmid',
        toolCalls: []
      })
    ]
    for (const summary of summaries) {
      const list = [
        msg({ id: 'u1', role: 'user', content: 'task' }),
        msg({ id: 'a1', role: 'assistant', content: 'edit', toolCalls: [editBefore] }),
        summary,
        msg({ id: 'a2', role: 'assistant', content: 'write', toolCalls: [writeAfter] })
      ]
      expect(collectLeadTurnStarts(list).map(item => item.turnId)).toEqual(['u1'])
      const byTurn = fileChangesByTurn(list)
      expect([...byTurn.keys()]).toEqual(['u1'])
      expect(byTurn.get('u1')?.map(f => f.path)).toEqual(['/ws/a.ts', '/ws/b.ts'])
      const last = lastTurnFileChanges(list)
      expect(last?.turnId).toBe('u1')
      expect(last?.files.map(f => f.path)).toEqual(['/ws/a.ts', '/ws/b.ts'])
    }
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

  it('does not attach previous-turn scoped writes to the next active turn', () => {
    const previousWrite = tc({
      id: 'e1',
      name: 'file_edit',
      result: JSON.stringify({
        path: '/ws/from-sub.ts',
        success: true,
        replaced: 1,
        stats: { adds: 770, dels: 18 }
      })
    })
    const list = [
      msg({ id: 'u1', role: 'user', content: 'first' }),
      msg({ id: 'a1', role: 'assistant', content: 'done' }),
      msg({ id: 'u2', role: 'user', content: 'deepen' }),
      msg({ id: 'a2', role: 'assistant', content: '', status: 'streaming' })
    ]
    const extra = [
      msg({
        id: 'sub_asst',
        role: 'assistant',
        content: '',
        anchorMessageId: 'a1',
        toolCalls: [previousWrite]
      }),
      msg({
        id: 'nested_asst',
        role: 'assistant',
        content: '',
        anchorMessageId: 'sub_asst',
        toolCalls: [
          tc({
            id: 'e2',
            name: 'file_write',
            result: JSON.stringify({ path: '/ws/nested.ts', success: true, bytesWritten: 4 })
          })
        ]
      })
    ]
    const starts = collectLeadTurnStarts(list)
    const frozen = frozenFileChangesFromStarts(list, starts, null, extra)
    expect(frozen.map.get('u1')?.map(f => f.path)).toEqual(['/ws/from-sub.ts', '/ws/nested.ts'])
    expect(frozen.map.has('u2')).toBe(false)

    const active = resolveActiveTurnFileChanges(list, 'u2', starts[1]!.start, null, extra)
    expect(active.files).toEqual([])
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
