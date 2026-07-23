import { describe, expect, it } from 'vitest'
import { buildConversationTurns, shouldAutoExpandTurn, turnContains } from './conversationTurns'

type Entry = {
  id: string
  role?: 'user' | 'assistant' | 'process'
  status?: 'done' | 'streaming' | 'error' | 'cancelled'
  summary?: boolean
  delivery?: boolean
}

const classifier = {
  key: (entry: Entry) => entry.id,
  userMessageId: (entry: Entry) => entry.role === 'user' ? entry.id : null,
  isActive: (entry: Entry) => entry.status === 'streaming',
  isFailed: (entry: Entry) => entry.status === 'error',
  isCancelled: (entry: Entry) => entry.status === 'cancelled',
  isSummary: (entry: Entry) => entry.summary === true,
  isDelivery: (entry: Entry) => entry.delivery === true
}

describe('conversation turns', () => {
  it('collapses a completed turn to user, summaries, and the final delivery', () => {
    const entries: Entry[] = [
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'process-1', role: 'process', status: 'done' },
      { id: 'summary', role: 'assistant', status: 'done', summary: true },
      { id: 'process-2', role: 'process', status: 'done' },
      { id: 'draft', role: 'assistant', status: 'done', delivery: true },
      { id: 'final', role: 'assistant', status: 'done', delivery: true }
    ]

    const [turn] = buildConversationTurns(entries, classifier)
    expect(turn?.id).toBe('u1')
    expect(turn?.state).toBe('completed')
    expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1', 'summary', 'final'])
    expect(turn?.hiddenCount).toBe(3)
  })

  it.each(['streaming', 'error', 'cancelled'] as const)(
    'keeps %s turns expanded',
    status => {
      const entries: Entry[] = [
        { id: 'u1', role: 'user', status: 'done' },
        { id: 'process', role: 'process', status },
        { id: 'answer', role: 'assistant', status: 'done', delivery: true }
      ]
      const [turn] = buildConversationTurns(entries, classifier)
      if (status === 'streaming') {
        expect(turn?.collapsedEntries).toEqual(entries)
        expect(turn?.hiddenCount).toBe(0)
      } else {
        expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1', 'answer'])
        expect(turn?.hiddenCount).toBe(1)
      }
    }
  )

  it('creates a collapse row for a cancelled turn with completed work entries', () => {
    const entries: Entry[] = [
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'tool-1', role: 'process', status: 'done' },
      { id: 'tool-2', role: 'process', status: 'done' },
      { id: 'stopped', role: 'assistant', status: 'cancelled', delivery: true }
    ]

    const [turn] = buildConversationTurns(entries, classifier)
    expect(turn?.state).toBe('cancelled')
    expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1', 'stopped'])
    expect(turn?.hiddenCount).toBe(2)
  })

  it('uses each user message as a stable turn id and finds hidden targets', () => {
    const turns = buildConversationTurns<Entry>([
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'hidden', role: 'process', status: 'done' },
      { id: 'final-1', role: 'assistant', status: 'done', delivery: true },
      { id: 'u2', role: 'user', status: 'done' },
      { id: 'active', role: 'assistant', status: 'streaming' }
    ], classifier)

    expect(turns.map(turn => turn.id)).toEqual(['u1', 'u2'])
    expect(turnContains(turns[0]!, entry => entry.id === 'hidden')).toBe(true)
  })

  it('auto-expands only the latest terminal turn with hidden work', () => {
    const terminalTurns = buildConversationTurns<Entry>([
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'hidden-1', role: 'process', status: 'done' },
      { id: 'final-1', role: 'assistant', status: 'done', delivery: true }
    ], classifier)

    expect(shouldAutoExpandTurn(terminalTurns, 'u1')).toBe(true)

    const nextTurn = buildConversationTurns<Entry>([
      ...terminalTurns[0]!.entries,
      { id: 'u2', role: 'user', status: 'done' },
      { id: 'active-2', role: 'assistant', status: 'streaming' }
    ], classifier)

    expect(shouldAutoExpandTurn(nextTurn, 'u1')).toBe(false)
    expect(shouldAutoExpandTurn(nextTurn, 'u2')).toBe(false)
  })
})
