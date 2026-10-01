import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import { EMPTY_CHAT_RETENTION, measureChatRetention } from './chatRetention'

function message(over: Partial<ChatMessage> & { id: string }): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    ...over
  }
}

function toolCall(over: Partial<ToolCall> = {}): ToolCall {
  return {
    id: 'call-1',
    name: 'read_file',
    arguments: '{}',
    status: 'success',
    ...over
  }
}

describe('measureChatRetention', () => {
  it('sums content, reasoning, thoughts and each tool call body', () => {
    const lead = [
      message({ id: 'm1', content: 'hello' }), // 5
      message({
        id: 'm2',
        content: 'world', // 5
        reasoning: 'because', // 7
        thoughts: 'hmm', // 3
        toolCalls: [
          toolCall({ id: 'c1', arguments: '{"a":1}', result: 'ok' }), // 7 + 2
          toolCall({ id: 'c2', arguments: '{"b":2}' }) // 7
        ]
      })
    ]

    const retention = measureChatRetention(lead, [])

    expect(retention.leadMessages).toBe(2)
    expect(retention.leadChars).toBe(5 + 5 + 7 + 3 + 7 + 2 + 7)
    expect(retention.scopedRows).toBe(0)
    expect(retention.scopedChars).toBe(0)
  })

  it('ignores fields that are missing, without dropping the message', () => {
    const lead = [
      message({ id: 'm1', content: 'only content' }),
      // No reasoning, no thoughts, no toolCalls at all.
      message({ id: 'm2', content: 'x' }),
      // A tool call with no result yet (still running).
      message({ id: 'm3', toolCalls: [toolCall({ arguments: '{}' })] })
    ]

    const retention = measureChatRetention(lead, [])

    expect(retention.leadMessages).toBe(3)
    expect(retention.leadChars).toBe('only content'.length + 1 + 2)
    expect(Number.isNaN(retention.leadChars)).toBe(false)
  })

  it('keeps the lead transcript and the scoped rows apart', () => {
    const retention = measureChatRetention(
      [message({ id: 'm1', content: 'lead' })],
      [message({ id: 'r1', content: 'scoped one' }), message({ id: 'r2', content: 'two' })]
    )

    expect(retention.leadMessages).toBe(1)
    expect(retention.leadChars).toBe(4)
    expect(retention.scopedRows).toBe(2)
    expect(retention.scopedChars).toBe('scoped one'.length + 3)
  })

  it('counts a tool body only while its text is still held', () => {
    const retention = measureChatRetention(
      [
        message({
          id: 'm1',
          toolCalls: [
            toolCall({ id: 'c1', arguments: '{"a":1}', result: 'ok' }),
            // Evicted: the text is gone, so this is neither chars nor a body.
            toolCall({ id: 'c2', arguments: '', result: '', bodyEvicted: true }),
            // Empty but not evicted: nothing retained, so not a body either.
            toolCall({ id: 'c3', arguments: '', result: '' })
          ]
        })
      ],
      []
    )

    expect(retention.toolBodies).toBe(1)
    expect(retention.leadChars).toBe(7 + 2)
  })

  it('counts an aside only while its text is still held', () => {
    const retention = measureChatRetention(
      [
        message({ id: 'm1', reasoning: 'why' }),
        message({ id: 'm2', thoughts: 'hmm' }),
        // Cleared together by `asideEvicted`.
        message({ id: 'm3', reasoning: '', thoughts: '', asideEvicted: true }),
        message({ id: 'm4', content: 'no aside at all' })
      ],
      [message({ id: 'r1', reasoning: 'scoped why' })]
    )

    expect(retention.asides).toBe(3)
    expect(retention.leadChars).toBe(3 + 3 + 0 + 'no aside at all'.length)
    expect(retention.scopedChars).toBe('scoped why'.length)
  })

  it('reports zeros for an empty conversation', () => {
    expect(measureChatRetention([], [])).toEqual(EMPTY_CHAT_RETENTION)
  })
})
