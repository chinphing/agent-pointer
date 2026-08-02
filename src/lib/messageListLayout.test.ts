import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  buildMessageListLayout,
  entryKey,
  matchingCompletedPrefixCount,
  messageStructureFingerprint,
  splitMessageTurnSegments,
  turnSegmentFingerprint,
  type FlattenDeps,
  type MessageListLayoutCache
} from './messageListLayout'

function user(id: string, content = 'hi'): ChatMessage {
  return { id, role: 'user', content, status: 'done', createdAt: 1 }
}

function assistant(
  id: string,
  content: string,
  status: ChatMessage['status'] = 'done'
): ChatMessage {
  return { id, role: 'assistant', content, status, createdAt: 2, toolCalls: [] }
}

const emptyDeps: FlattenDeps = {
  boardsForMessage: () => [],
  visibleToolCallsFor: message => message.toolCalls ?? [],
  shouldShowGlue: () => true
}

describe('splitMessageTurnSegments', () => {
  it('anchors each user message as a turn boundary', () => {
    const messages = [user('u1'), assistant('a1', 'one'), user('u2'), assistant('a2', 'two')]
    expect(splitMessageTurnSegments(messages).map(s => s.id)).toEqual(['u1', 'u2'])
    expect(splitMessageTurnSegments(messages)[0]).toMatchObject({ start: 0, end: 2 })
    expect(splitMessageTurnSegments(messages)[1]).toMatchObject({ start: 2, end: 4 })
  })

  it('does not anchor empty-response retry injects as a new turn', () => {
    const messages = [
      user('u1', 'task'),
      assistant('a1', '', 'done'),
      user(
        'fmt_retry_abc',
        '你的上一次回复为空，既没有文本内容也没有工具调用。（异常重试 1/3）'
      ),
      assistant('a2', 'ok')
    ]
    expect(splitMessageTurnSegments(messages).map(s => s.id)).toEqual(['u1'])
    expect(splitMessageTurnSegments(messages)[0]).toMatchObject({ start: 0, end: 4 })
  })
})

describe('messageStructureFingerprint', () => {
  it('ignores content text changes', () => {
    const a = assistant('a1', 'hello')
    const b = assistant('a1', 'hello world, much longer')
    expect(messageStructureFingerprint(a, emptyDeps)).toBe(
      messageStructureFingerprint(b, emptyDeps)
    )
  })

  it('changes when status changes', () => {
    const a = assistant('a1', 'hello', 'streaming')
    const b = assistant('a1', 'hello', 'done')
    expect(messageStructureFingerprint(a, emptyDeps)).not.toBe(
      messageStructureFingerprint(b, emptyDeps)
    )
  })
})

describe('buildMessageListLayout', () => {
  it('reuses completed prefix turns while rebuilding the streaming tail', () => {
    const base = [user('u1'), assistant('a1', 'done reply'), user('u2')]
    const first = buildMessageListLayout({
      conversationId: 'c1',
      messages: [...base, assistant('a2', 'partial', 'streaming')],
      deps: emptyDeps,
      cache: null
    })
    expect(first.turns).toHaveLength(2)
    expect(first.reusablePrefixTurns).toBe(0)

    const second = buildMessageListLayout({
      conversationId: 'c1',
      messages: [...base, assistant('a2', 'partial and more', 'streaming')],
      deps: emptyDeps,
      cache: first.cache
    })
    expect(second.reusablePrefixTurns).toBe(1)
    expect(second.turns).toHaveLength(2)
    expect(second.turns[0]!.id).toBe('u1')
    expect(second.turns[0]!.state).toBe('completed')
    expect(second.turns[0]!.entries.map(e => entryKey(e))).toEqual(
      first.cache.turns[0]!.entries.map(e => entryKey(e))
    )
    expect(second.turns[1]!.state).toBe('active')
  })

  it('invalidates prefix when an earlier turn structure changes', () => {
    const messages = [user('u1'), assistant('a1', 'one'), user('u2'), assistant('a2', 'two')]
    const first = buildMessageListLayout({
      conversationId: 'c1',
      messages,
      deps: emptyDeps,
      cache: null
    })

    const mutated: ChatMessage[] = [
      user('u1'),
      { ...assistant('a1', 'one'), status: 'error', errorMessage: 'boom' },
      user('u2'),
      assistant('a2', 'two')
    ]
    const second = buildMessageListLayout({
      conversationId: 'c1',
      messages: mutated,
      deps: emptyDeps,
      cache: first.cache
    })
    expect(second.reusablePrefixTurns).toBe(0)
    expect(second.turns[0]!.state).toBe('failed')
  })

  it('never treats the last turn as reusable prefix', () => {
    const fingerprints = [
      turnSegmentFingerprint([user('u1'), assistant('a1', 'x')], 0, 2, emptyDeps)
    ]
    const cache: MessageListLayoutCache = {
      conversationId: 'c1',
      fingerprints,
      entries: [],
      turns: [{
        id: 'u1',
        entries: [],
        state: 'completed',
        collapsedEntries: [],
        hiddenCount: 0
      }]
    }
    expect(matchingCompletedPrefixCount('c1', fingerprints, cache)).toBe(0)
  })
})
