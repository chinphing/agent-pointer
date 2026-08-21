import { describe, expect, it } from 'vitest'
import { buildConversationTurns, shouldAutoExpandTurn, turnContains } from './conversationTurns'

type Entry = {
  id: string
  role?: 'user' | 'assistant' | 'process'
  status?: 'done' | 'streaming' | 'error' | 'cancelled'
  summary?: boolean
  delivery?: boolean
  interactive?: boolean
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
    'keeps %s turns expanded when collapseActiveTurns is off',
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

  it('collapses active turns when collapseActiveTurns is on', () => {
    const entries: Entry[] = [
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'process', role: 'process', status: 'streaming' },
      { id: 'answer', role: 'assistant', status: 'done', delivery: true }
    ]
    const [turn] = buildConversationTurns(entries, classifier, { collapseActiveTurns: true })
    expect(turn?.state).toBe('active')
    expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1', 'answer'])
    expect(turn?.hiddenCount).toBe(1)
  })

  it('omits delivery on active turns when omitDeliveryWhileActive is on', () => {
    const entries: Entry[] = [
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'process', role: 'process', status: 'streaming' },
      { id: 'draft', role: 'assistant', status: 'done', delivery: true }
    ]
    const [turn] = buildConversationTurns(entries, classifier, {
      collapseActiveTurns: true,
      omitDeliveryWhileActive: true
    })
    expect(turn?.state).toBe('active')
    expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1'])
    expect(turn?.hiddenCount).toBe(2)
  })

  it('keeps interactive entries on active turns even when delivery is omitted', () => {
    const entries: Entry[] = [
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'draft', role: 'assistant', status: 'done', delivery: true },
      { id: 'ask', role: 'process', status: 'streaming', interactive: true }
    ]
    const interactiveClassifier = {
      ...classifier,
      isInteractive: (entry: Entry) => entry.interactive === true
    }
    const [turn] = buildConversationTurns(entries, interactiveClassifier, {
      collapseActiveTurns: true,
      omitDeliveryWhileActive: true
    })
    expect(turn?.collapsedEntries.map(entry => entry.id)).toEqual(['u1', 'ask'])
    expect(turn?.hiddenCount).toBe(1)
  })

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

  it('attaches turn headers to the next user turn', () => {
    const headerClassifier = {
      ...classifier,
      userMessageId: (entry: Entry) => entry.role === 'user' ? entry.id : null,
      isTurnHeader: (entry: Entry) => entry.id === 'chip'
    }
    const turns = buildConversationTurns<Entry>([
      { id: 'u1', role: 'user', status: 'done' },
      { id: 'final-1', role: 'assistant', status: 'done', delivery: true },
      { id: 'chip', role: 'assistant', status: 'done', summary: true },
      { id: 'u2', role: 'user', status: 'done' },
      { id: 'final-2', role: 'assistant', status: 'done', delivery: true }
    ], headerClassifier)

    expect(turns.map(turn => turn.id)).toEqual(['u1', 'u2'])
    expect(turns[0]!.entries.map(e => e.id)).toEqual(['u1', 'final-1'])
    expect(turns[1]!.entries.map(e => e.id)).toEqual(['chip', 'u2', 'final-2'])
    expect(turns[1]!.collapsedEntries.map(e => e.id)).toEqual(['chip', 'u2', 'final-2'])
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

    // Active last turn is never auto-expanded; previous turn is no longer last.
    expect(shouldAutoExpandTurn(nextTurn, 'u1')).toBe(false)
    expect(shouldAutoExpandTurn(nextTurn, 'u2')).toBe(false)
  })
})
