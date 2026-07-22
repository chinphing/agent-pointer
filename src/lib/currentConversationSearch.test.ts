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
