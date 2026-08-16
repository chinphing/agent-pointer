import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import { fileChangesByTurn, lastTurnFileChanges } from './lastTurnFileChanges'

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
})
