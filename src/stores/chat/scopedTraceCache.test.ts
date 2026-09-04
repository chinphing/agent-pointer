import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../../types/chat'
import { createScopedTraceCache } from './scopedTraceCache'

function scopedMsg(over: Partial<ChatMessage> & { id: string }): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 0,
    anchorMessageId: 'anchor-1',
    traceId: 'trace-a',
    ...over
  }
}

describe('scopedTraceCache', () => {
  it('touchScopedMessage publishes incremental live signal', () => {
    const cache = createScopedTraceCache()
    const msg = scopedMsg({ id: 'm1', content: 'a' })
    cache.rebuild('conv-1', [msg])
    cache.touchScopedMessage('conv-1', msg)
    expect(cache.getLiveSignal('conv-1', 'trace-a')).toContain('|')
    const before = cache.getLiveSignal('conv-1', 'trace-a')
    msg.content = 'ab'
    cache.touchScopedMessage('conv-1', msg)
    expect(cache.getLiveSignal('conv-1', 'trace-a')).not.toBe(before)
  })

  it('clearAll drops indexes and live signals', () => {
    const cache = createScopedTraceCache()
    const msg = scopedMsg({ id: 'm1', content: 'x' })
    cache.rebuild('conv-1', [msg])
    cache.touchScopedMessage('conv-1', msg)
    cache.clearAll()
    expect(cache.getLiveSignal('conv-1', 'trace-a')).toBe('')
    expect(cache.getMessages('conv-1', [msg], 'anchor-1', 'trace-a')).toEqual([msg])
  })
})
