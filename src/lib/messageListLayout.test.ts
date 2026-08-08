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

describe('collapsed turn projection', () => {
  it('keeps only final content and strips trailing tools on completed turns', () => {
    const toolOnly: ChatMessage = {
      id: 't1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 3,
      toolCalls: [{
        id: 'tc1',
        name: 'skill_read',
        status: 'success',
        arguments: '{}'
      }]
    }
    const result = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', '报销'),
        assistant('a1', '好的，我先读取技能'),
        toolOnly
      ],
      deps: emptyDeps,
      cache: null
    })
    const turn = result.turns[0]!
    expect(turn.state).toBe('completed')
    expect(turn.hiddenCount).toBeGreaterThan(0)
    const collapsed = turn.collapsedEntries
    expect(collapsed.map(e => e.type)).toEqual(['message', 'message'])
    const delivery = collapsed[1]!
    expect(delivery.type).toBe('message')
    if (delivery.type !== 'message') return
    expect(delivery.message.content).toBe('好的，我先读取技能')
    expect(delivery.contentOnly).toBe(true)
    expect(delivery.trailingToolGroups).toBeUndefined()
  })

  it('omits intermediate delivery while the turn is still active', () => {
    const result = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', '报销'),
        assistant('a1', '好的，我先处理', 'streaming')
      ],
      deps: emptyDeps,
      cache: null,
      collapseActiveTurns: true
    })
    const turn = result.turns[0]!
    expect(turn.state).toBe('active')
    expect(turn.collapsedEntries.map(e =>
      e.type === 'message' ? e.message.id : e.type
    )).toEqual(['u1'])
    expect(turn.hiddenCount).toBeGreaterThan(0)
  })

  it('keeps pending ask_user visible on an active collapsed turn', () => {
    const ask: ChatMessage = {
      id: 'ask1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 3,
      toolCalls: [{
        id: 'tc-ask',
        name: 'ask_user',
        status: 'pending',
        arguments: '{"prompt":"选哪个？"}'
      }]
    }
    const result = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', '报销'),
        assistant('a1', '请选择经费', 'streaming'),
        ask
      ],
      deps: emptyDeps,
      cache: null,
      collapseActiveTurns: true
    })
    const turn = result.turns[0]!
    expect(turn.state).toBe('active')
    // ask_user is attached as trailingToolGroups on the preceding assistant row.
    const host = turn.collapsedEntries.find(
      (e): e is Extract<typeof e, { type: 'message' }> =>
        e.type === 'message' && e.message.id === 'a1'
    )
    expect(host?.contentOnly).toBe(true)
    expect(host?.trailingToolGroups?.map(g => g.id)).toEqual(['ask1'])
    expect(host?.trailingToolGroups?.[0]?.toolCalls.map(tc => tc.name)).toEqual(['ask_user'])
  })

  it('keeps a running task board in the collapsed active projection', () => {
    const boardDoc = {
      version: 4,
      task_id: 'tb1',
      meta: { goal: '办报销', status: 'running' },
      global_milestones: [
        { id: 'g_plan', title: '规划', status: 'done' as const },
        { id: 'g_exec', title: '执行', status: 'in_progress' as const }
      ]
    }
    const deps: FlattenDeps = {
      ...emptyDeps,
      boardsForMessage: messageId =>
        messageId === 'u1'
          ? [{ storeKey: 'main:u1', document: boardDoc, isActive: true }]
          : []
    }
    const result = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', '报销'),
        assistant('a1', '处理中', 'streaming')
      ],
      deps,
      cache: null,
      collapseActiveTurns: true
    })
    const turn = result.turns[0]!
    expect(turn.state).toBe('active')
    const boards = turn.collapsedEntries.filter(e => e.type === 'task_board')
    expect(boards).toHaveLength(1)
    expect(boards[0]).toMatchObject({
      type: 'task_board',
      storeKey: 'main:u1',
      isActive: true
    })
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
