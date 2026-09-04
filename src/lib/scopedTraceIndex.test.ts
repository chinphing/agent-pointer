import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  computeSubAgentLiveFingerprint,
  getScopedMessagesFromIndex,
  rebuildScopedTraceIndex,
  registerScopedMessageInIndex
} from './scopedTraceIndex'

function scopedMsg(over: Partial<ChatMessage> & { id: string }): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    anchorMessageId: 'anchor-1',
    traceId: 'trace-a',
    ...over
  }
}

describe('scopedTraceIndex', () => {
  it('rebuild indexes scoped rows by anchor + trace', () => {
    const index = rebuildScopedTraceIndex([
      scopedMsg({ id: 'm1', createdAt: 1, content: 'a' }),
      scopedMsg({ id: 'm2', createdAt: 2, traceId: 'trace-b', content: 'b' }),
      { id: 'root', role: 'assistant', content: 'root', status: 'done', createdAt: 0 }
    ])
    const a = getScopedMessagesFromIndex(index, 'anchor-1', 'trace-a')
    expect(a.map(m => m.id)).toEqual(['m1'])
    const b = getScopedMessagesFromIndex(index, 'anchor-1', 'trace-b')
    expect(b.map(m => m.id)).toEqual(['m2'])
  })

  it('filters by agentInstanceId when provided', () => {
    const index = rebuildScopedTraceIndex([
      scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'one' }),
      scopedMsg({ id: 'm2', agentInstanceId: 'inst-2', content: 'two' })
    ])
    expect(getScopedMessagesFromIndex(index, 'anchor-1', 'trace-a', 'inst-1').map(m => m.id)).toEqual(['m1'])
  })

  it('registerScopedMessageInIndex dedupes by message id', () => {
    const index = rebuildScopedTraceIndex([])
    const msg = scopedMsg({ id: 'm1', createdAt: 5 })
    registerScopedMessageInIndex(index, msg)
    registerScopedMessageInIndex(index, msg)
    expect(getScopedMessagesFromIndex(index, 'anchor-1', 'trace-a')).toHaveLength(1)
  })

  it('computeSubAgentLiveFingerprint tracks text and tool status', () => {
    const scoped = [
      scopedMsg({
        id: 'm1',
        content: 'hello',
        toolCalls: [{ id: 'tc1', name: 'read', status: 'running', args: {} }]
      })
    ]
    const fp1 = computeSubAgentLiveFingerprint(scoped)
    scoped[0].content = 'hello world'
    const fp2 = computeSubAgentLiveFingerprint(scoped)
    expect(fp1).not.toBe(fp2)
    scoped[0].toolCalls![0].status = 'success'
    const fp3 = computeSubAgentLiveFingerprint(scoped)
    expect(fp2).not.toBe(fp3)
  })
})
