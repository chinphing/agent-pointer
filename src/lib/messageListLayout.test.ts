import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  buildMessageListLayout,
  coalesceToolRunItems,
  entryKey,
  flattenConversationMessages,
  insertContextCompressingMarker,
  matchingCompletedPrefixCount,
  messageStructureFingerprint,
  splitMessageTurnSegments,
  toolRunHostMessage,
  turnElapsedHostIndex,
  turnSegmentFingerprint,
  withTurnElapsedFollowingSpacing,
  type FlattenDeps,
  type MessageListLayoutCache,
  type ToolRunGroup
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

  it('attaches compression summaries to the next user turn, not the previous reply', () => {
    const summary: ChatMessage = {
      id: 'sum',
      role: 'user',
      content: '[Conversation summary (auto-compression)]\nbody',
      status: 'done',
      createdAt: 3
    }
    const messages = [user('u1', 'old'), assistant('a1', 'done'), summary, user('u2', 'keep')]
    expect(splitMessageTurnSegments(messages).map(s => s.id)).toEqual(['u1', 'u2'])
    expect(splitMessageTurnSegments(messages)[0]).toMatchObject({ start: 0, end: 2 })
    expect(splitMessageTurnSegments(messages)[1]).toMatchObject({ start: 2, end: 4 })

    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages,
      deps: emptyDeps,
      cache: null
    })
    expect(layout.turns.map(t => t.id)).toEqual(['u1', 'u2'])
    expect(layout.turns[0]!.entries.map(e => e.type === 'message' ? e.message.id : e.type)).toEqual(['u1', 'a1'])
    expect(layout.turns[1]!.entries.map(e => e.type === 'message' ? e.message.id : e.type)).toEqual(['sum', 'u2'])
    expect(layout.turns[1]!.collapsedEntries.map(e =>
      e.type === 'message' ? e.message.id : e.type
    )).toEqual(['sum', 'u2'])
  })

  it('keeps in-run assistant summaries inside the same user turn', () => {
    const summary: ChatMessage = {
      id: 'sum',
      role: 'assistant',
      content: '[Conversation summary (auto-compression)]\nmid-turn',
      status: 'done',
      createdAt: 3,
      toolCalls: []
    }
    const messages = [
      user('u1', 'task'),
      assistant('a1', 'step'),
      summary,
      assistant('a2', 'done')
    ]
    expect(splitMessageTurnSegments(messages).map(s => s.id)).toEqual(['u1'])
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages,
      deps: emptyDeps,
      cache: null
    })
    expect(layout.turns.map(t => t.id)).toEqual(['u1'])
    expect(layout.turns[0]!.entries.map(e => e.type === 'message' ? e.message.id : e.type)).toEqual([
      'u1',
      'a1',
      'sum',
      'a2'
    ])
    expect(layout.turns[0]!.collapsedEntries.map(e =>
      e.type === 'message' ? e.message.id : e.type
    )).toEqual(['u1', 'a2'])
  })

  it('keeps a mid-turn prefix summary visible when the turn is collapsed', () => {
    const summary: ChatMessage = {
      id: 'sum',
      role: 'user',
      content: '[Conversation summary (auto-compression)]\nolder prefix',
      status: 'done',
      createdAt: 3
    }
    const messages = [
      user('u1', 'task'),
      assistant('a1', 'step'),
      summary,
      assistant('a2', 'done')
    ]
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages,
      deps: emptyDeps,
      cache: null
    })
    expect(layout.turns.map(t => t.id)).toEqual(['u1'])
    expect(layout.turns[0]!.entries.map(e => e.type === 'message' ? e.message.id : e.type)).toEqual([
      'u1',
      'a1',
      'sum',
      'a2'
    ])
    expect(layout.turns[0]!.collapsedEntries.map(e =>
      e.type === 'message' ? e.message.id : e.type
    )).toEqual(['u1', 'sum', 'a2'])
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

  it('keeps in-flight run_subagent so nested coder ask_user can surface when collapsed', () => {
    const host: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 2,
      toolCalls: [{
        id: 'tc-sub',
        name: 'run_subagent',
        status: 'running',
        arguments: '{"agent":"coder","goal":"改代码"}'
      }],
      agentTrace: [{
        id: 'task:coder',
        name: 'coder',
        role: 'coder',
        agentId: 'coder',
        depth: 1,
        status: 'running',
        parentToolCallId: 'tc-sub',
        session: {
          toolCalls: [{
            id: 'tc-ask',
            name: 'ask_user',
            status: 'pending',
            arguments: '{"question":"选哪个？","options":[{"label":"A"},{"label":"B"}]}'
          }],
          stats: {
            searchCount: 0,
            readCount: 0,
            writeCount: 0,
            terminalCount: 0,
            webSearchCount: 0,
            skillCount: 0,
            mediaCount: 0,
            mouseCount: 0,
            inputCount: 0,
            otherCount: 0
          },
          collapsed: false,
          userExpanded: false
        }
      }]
    }
    const result = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', '帮我改'),
        host
      ],
      deps: emptyDeps,
      cache: null,
      collapseActiveTurns: true
    })
    const turn = result.turns[0]!
    expect(turn.state).toBe('active')
    const kept = turn.collapsedEntries.find(
      (e): e is Extract<typeof e, { type: 'message' }> =>
        e.type === 'message' && e.message.id === 'a1'
    )
    expect(kept).toBeTruthy()
    expect(kept?.contentOnly).toBe(true)
    // Host row must remain so SubAgentFrame can mount under after-tool.
    const tools =
      kept?.message.toolCalls
      ?? []
    // contentOnly filtering of body tools happens in AgentMessageBody;
    // projection must still keep the message (and not strip run_subagent from data).
    expect(tools.some(tc => tc.name === 'run_subagent' && tc.status === 'running')).toBe(true)
    expect(kept?.message.agentTrace?.[0]?.status).toBe('running')
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

describe('insertContextCompressingMarker', () => {
  it('places the marker immediately before the keep-window message', () => {
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages: [user('u1', 'old'), user('u2', 'keep'), assistant('a2', 'ok')],
      deps: emptyDeps,
      cache: null
    })
    const turns = insertContextCompressingMarker(layout.turns, 'u2', undefined, '正在压缩较早记录')
    const keys = turns.flatMap(turn => turn.entries.map(entry => entryKey(entry)))
    const split = keys.indexOf('context-compressing')
    expect(split).toBeGreaterThanOrEqual(0)
    expect(keys[split + 1]).toBe('message-u2')
  })

  it('does not pin the fallback marker after the last delivery', () => {
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages: [user('u1', 'q'), assistant('a1', 'reply')],
      deps: emptyDeps,
      cache: null
    })
    const turns = insertContextCompressingMarker(
      layout.turns,
      'missing-keep-id',
      undefined,
      '正在压缩较早记录'
    )
    const keys = turns[0]!.collapsedEntries.map(entry => entryKey(entry))
    const split = keys.indexOf('context-compressing')
    expect(split).toBeGreaterThanOrEqual(0)
    expect(keys[split + 1]).toBe('message-a1')
  })

  it('keeps the compressing marker before the delivery when the cut is a tool', () => {
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
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages: [user('u1', 'q'), assistant('a1', 'step'), toolOnly, assistant('a2', 'done')],
      deps: emptyDeps,
      cache: null
    })
    const turns = insertContextCompressingMarker(
      layout.turns,
      't1',
      undefined,
      '正在压缩较早记录'
    )
    const collapsedKeys = turns[0]!.collapsedEntries.map(entry => entryKey(entry))
    const collapsedSplit = collapsedKeys.indexOf('context-compressing')
    expect(collapsedSplit).toBeGreaterThanOrEqual(0)
    expect(collapsedKeys[collapsedSplit + 1]).toBe('message-a2')
    const expandedKeys = turns[0]!.entries.map(entry => entryKey(entry))
    const split = expandedKeys.indexOf('context-compressing')
    expect(split).toBeGreaterThanOrEqual(0)
    expect(expandedKeys.indexOf('message-a2')).toBeGreaterThan(split)
  })

  it('falls back to the first visible turn when the keep id is off this page', () => {
    const layout = buildMessageListLayout({
      conversationId: 'c1',
      messages: [
        user('u1', 'older'),
        assistant('a1', 'done'),
        user('u2', 'live'),
        assistant('a2', 'running', 'streaming')
      ],
      deps: emptyDeps,
      cache: null
    })
    const turns = insertContextCompressingMarker(
      layout.turns,
      'keep-not-on-page',
      undefined,
      '正在压缩较早记录'
    )
    expect(turns).toHaveLength(2)
    const firstKeys = turns[0]!.collapsedEntries.map(entry => entryKey(entry))
    const lastKeys = turns[1]!.collapsedEntries.map(entry => entryKey(entry))
    expect(firstKeys).toContain('context-compressing')
    expect(lastKeys).not.toContain('context-compressing')
    expect(firstKeys[firstKeys.indexOf('context-compressing') + 1]).toBe('message-a1')
  })
})

function toolGroup(id: string, status: ChatMessage['status'] = 'done'): ToolRunGroup {
  return {
    id,
    toolCalls: [{ id: `tc-${id}`, name: 'file_read', status: 'success', arguments: '{}' }],
    message: {
      id,
      role: 'assistant',
      content: '',
      status,
      createdAt: 1,
      toolCalls: []
    }
  }
}

describe('coalesceToolRunItems', () => {
  it('merges adjacent tool-only rounds and splits on glue', () => {
    const glue: ChatMessage = {
      id: 'glue',
      role: 'user',
      content: 'retry',
      status: 'done',
      createdAt: 2
    }
    const blocks = coalesceToolRunItems([
      { kind: 'tools', group: toolGroup('a') },
      { kind: 'tools', group: toolGroup('b') },
      { kind: 'glue', message: glue },
      { kind: 'tools', group: toolGroup('c') }
    ])
    expect(blocks.map(b => b.kind)).toEqual(['tools', 'glue', 'tools'])
    expect(blocks[0]?.kind === 'tools' && blocks[0].groups.map(g => g.id)).toEqual(['a', 'b'])
    expect(blocks[2]?.kind === 'tools' && blocks[2].groups.map(g => g.id)).toEqual(['c'])
  })
})

describe('toolRunHostMessage', () => {
  it('prefers the streaming round', () => {
    const host = toolRunHostMessage([
      toolGroup('done', 'done'),
      toolGroup('live', 'streaming')
    ])
    expect(host?.id).toBe('live')
  })
})

describe('thinking shell in process-tool runs', () => {
  it('folds a thinking shell into the preceding tool-only run', () => {
    const toolOnly: ChatMessage = {
      id: 't1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 2,
      toolCalls: [{ id: 'tc1', name: 'terminal', status: 'success', arguments: '{}' }]
    }
    const thinking: ChatMessage = {
      id: 'th',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 3,
      toolCalls: []
    }
    const entries = flattenConversationMessages(
      [user('u1'), toolOnly, thinking],
      emptyDeps
    )
    expect(entries.map(e => e.type)).toEqual(['message', 'tool_run'])
    const run = entries[1]
    expect(run?.type).toBe('tool_run')
    if (run?.type !== 'tool_run') return
    const groups = run.items
      .filter((i): i is { kind: 'tools'; group: ToolRunGroup } => i.kind === 'tools')
      .map(i => i.group)
    expect(toolRunHostMessage(groups)?.id).toBe('th')
  })

  it('keeps first-round thinking as its own message when no tools exist yet', () => {
    const thinking: ChatMessage = {
      id: 'th',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 2,
      toolCalls: []
    }
    const entries = flattenConversationMessages([user('u1'), thinking], emptyDeps)
    expect(entries.map(e => e.type)).toEqual(['message', 'message'])
    expect(entries[1]?.type === 'message' && entries[1].message.id).toBe('th')
  })

  it('folds a thinking shell under a host that already has its own tools', () => {
    const host: ChatMessage = {
      id: 'host',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 2,
      toolCalls: [{ id: 'tc1', name: 'run_subagent', status: 'success', arguments: '{}' }],
      agentTrace: [{ id: 'tr1', name: 'coder', role: 'coder', status: 'running', depth: 1 }]
    }
    const thinking: ChatMessage = {
      id: 'th',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 3,
      toolCalls: []
    }
    const entries = flattenConversationMessages([user('u1'), host, thinking], emptyDeps)
    expect(entries.map(e => e.type)).toEqual(['message', 'message'])
    const row = entries[1]
    expect(row?.type).toBe('message')
    if (row?.type !== 'message') return
    expect(row.message.id).toBe('host')
    expect(row.trailingToolGroups?.some(g => g.id === 'th')).toBe(true)
  })

  it('drops the thinking shell from the tool run once reply content arrives', () => {
    const toolOnly: ChatMessage = {
      id: 't1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 2,
      toolCalls: [{ id: 'tc1', name: 'terminal', status: 'success', arguments: '{}' }]
    }
    const thinking: ChatMessage = {
      id: 'th',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 3,
      toolCalls: []
    }
    const reply: ChatMessage = {
      id: 'a2',
      role: 'assistant',
      content: '计算完成。',
      status: 'streaming',
      createdAt: 4,
      toolCalls: []
    }
    const entries = flattenConversationMessages(
      [user('u1'), toolOnly, thinking, reply],
      emptyDeps
    )
    expect(entries.map(e => e.type)).toEqual(['message', 'tool_run', 'message'])
    const run = entries[1]
    expect(run?.type).toBe('tool_run')
    if (run?.type !== 'tool_run') return
    const groupIds = run.items
      .filter((i): i is { kind: 'tools'; group: ToolRunGroup } => i.kind === 'tools')
      .map(i => i.group.id)
    expect(groupIds).toEqual(['t1'])
    expect(entries[2]?.type === 'message' && entries[2].message.id).toBe('a2')
  })
})

describe('turn elapsed following spacing', () => {
  it('hosts the elapsed row on the user message when there is no task board', () => {
    const entries = flattenConversationMessages(
      [user('u1'), assistant('a1', 'done')],
      emptyDeps
    )
    expect(turnElapsedHostIndex(entries, 'u1', 2, false)).toBe(0)
    expect(turnElapsedHostIndex(entries, 'u1', 0, false)).toBe(-1)
  })

  it('halves the user→assistant gap under the elapsed row', () => {
    expect(withTurnElapsedFollowingSpacing('mt-7', 1, 0)).toBe('mt-1.5')
    expect(withTurnElapsedFollowingSpacing('mt-7', 2, 1)).toBe('mt-1.5')
    expect(withTurnElapsedFollowingSpacing('mt-1.5', 1, 0)).toBe('mt-1.5')
    expect(withTurnElapsedFollowingSpacing('mt-7', 1, -1)).toBe('mt-7')
    expect(withTurnElapsedFollowingSpacing('mt-7', 2, 0)).toBe('mt-7')
  })
})
