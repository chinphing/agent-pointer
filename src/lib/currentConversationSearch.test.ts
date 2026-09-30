import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  findCurrentConversationMatches,
  searchableMessageText,
  searchableToolCallText
} from './currentConversationSearch'

function message(id: string, content: string, extra: Partial<ChatMessage> = {}): ChatMessage {
  return { id, role: 'assistant', content, status: 'done', createdAt: 1, ...extra }
}

describe('current conversation search', () => {
  it('matches message content case-insensitively and keeps transcript order', () => {
    const messages = [message('a', 'First Pointer result'), message('b', 'none'), message('c', 'pointer again')]
    expect(findCurrentConversationMatches(messages, 'POINTER')).toEqual([
      { messageId: 'a' },
      { messageId: 'c' }
    ])
  })

  it('returns the specific tool call whose visible text matched', () => {
    const messages = [
      message('a', '', { errorMessage: 'Network unavailable' }),
      message('b', '', {
        toolCalls: [
          { id: 'build', name: 'terminal', arguments: '{}', status: 'success', result: 'Build complete' },
          { id: 'deploy', name: 'terminal', arguments: '{}', status: 'success', result: 'Deploy complete' }
        ]
      })
    ]
    expect(findCurrentConversationMatches(messages, 'network')).toEqual([{ messageId: 'a' }])
    expect(findCurrentConversationMatches(messages, 'build complete')).toEqual([
      { messageId: 'b', toolCallId: 'build' }
    ])
    expect(searchableToolCallText(messages[1].toolCalls![0])).toContain('Build complete')
    expect(searchableMessageText(messages[1])).toContain('Deploy complete')
  })

  it('counts body and tool matches independently in display order', () => {
    const messages = [message('a', 'build summary', {
      toolCalls: [
        { id: 'one', name: 'terminal', arguments: '{"command":"build"}', status: 'success' },
        { id: 'two', name: 'terminal', arguments: '{}', status: 'success', result: 'build done' }
      ]
    })]
    expect(findCurrentConversationMatches(messages, 'build')).toEqual([
      { messageId: 'a' },
      { messageId: 'a', toolCallId: 'one' },
      { messageId: 'a', toolCallId: 'two' }
    ])
  })

  it('returns no matches for a blank query', () => {
    expect(findCurrentConversationMatches([message('a', 'text')], '   ')).toEqual([])
  })
})

describe('current conversation search over sub-agent scoped rows', () => {
  function scopedRow(id: string, over: Partial<ChatMessage> = {}): ChatMessage {
    return {
      id,
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      anchorMessageId: 'lead',
      agentInstanceId: 'inst-1',
      ...over
    }
  }

  it('reports a content hit against the frame anchor plus the scoped row id', () => {
    const rows = [scopedRow('round-1', { content: '中间结论：先看仓库' })]
    expect(findCurrentConversationMatches([], '中间结论', rows)).toEqual([
      { messageId: 'lead', contentMessageId: 'round-1' }
    ])
  })

  it('reports a tool hit on a scoped row with its tool call id', () => {
    const rows = [
      scopedRow('round-2', {
        toolCalls: [
          { id: 'deep-tool', name: 'terminal', arguments: '{}', status: 'success', result: 'deep output' }
        ]
      })
    ]
    expect(findCurrentConversationMatches([], 'deep output', rows)).toEqual([
      { messageId: 'lead', toolCallId: 'deep-tool' }
    ])
  })

  it('keeps transcript hits first and then scoped rows in row order', () => {
    const rows = [
      scopedRow('round-b', { content: 'hit b', createdAt: 5, position: 5 }),
      scopedRow('round-a', { content: 'hit a', createdAt: 2, position: 2 })
    ]
    expect(findCurrentConversationMatches([message('lead', 'hit transcript')], 'hit', rows)).toEqual([
      { messageId: 'lead' },
      { messageId: 'lead', contentMessageId: 'round-a' },
      { messageId: 'lead', contentMessageId: 'round-b' }
    ])
  })

  it('ignores rows without an anchor and keeps the default call shape', () => {
    const orphan = scopedRow('orphan', { content: 'orphan hit', anchorMessageId: '' })
    expect(findCurrentConversationMatches([message('a', 'hit')], 'hit', [orphan])).toEqual([
      { messageId: 'a' }
    ])
    expect(findCurrentConversationMatches([message('a', 'hit')], 'hit')).toEqual([
      { messageId: 'a' }
    ])
  })
})
