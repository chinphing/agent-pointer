import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import { leadThreadCompressionInsertIndex } from './compressionLayout'

function msg(
  id: string,
  role: ChatMessage['role'],
  extra: Partial<ChatMessage> = {}
): ChatMessage {
  return { id, role, content: id, status: 'done', createdAt: 1, ...extra }
}

describe('leadThreadCompressionInsertIndex', () => {
  it('inserts before the recorded lead keep id', () => {
    const messages = [msg('u1', 'user'), msg('a1', 'assistant')]
    expect(leadThreadCompressionInsertIndex(messages, 'a1')).toBe(1)
  })

  it('ignores a scoped child id and does not park after sub-agent rows', () => {
    const messages = [
      msg('u1', 'user'),
      msg('host', 'assistant'),
      msg('child', 'assistant', { anchorMessageId: 'host', traceId: 't1' }),
      msg('child2', 'assistant', { anchorMessageId: 'host', traceId: 't1' })
    ]
    expect(leadThreadCompressionInsertIndex(messages, 'child')).toBe(2)
    expect(leadThreadCompressionInsertIndex(messages, 'missing')).toBe(2)
  })

  it('falls back to the first kept lead row, skipping scoped children', () => {
    const messages = [
      msg('old', 'user'),
      msg('host', 'assistant'),
      msg('child', 'assistant', { anchorMessageId: 'host', traceId: 't1' }),
      msg('u2', 'user')
    ]
    expect(leadThreadCompressionInsertIndex(messages, 'gone', ['old', 'host'])).toBe(3)
  })
})
