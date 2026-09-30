import { beforeEach, describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import {
  ASK_USER_BANNER_LINGER_MS,
  createAskUserLinger,
  pendingAskUserToolCalls,
  resolveAskUserBannerView
} from './askUserBanner'
import {
  resetConversationScopedStoreForTests,
  useConversationScopedStore
} from './conversationScoped'

function askUser(id: string, status: ToolCall['status'] = 'pending'): ToolCall {
  return {
    id,
    name: 'ask_user',
    status,
    arguments: JSON.stringify({
      question: `Q-${id}`,
      options: [{ label: 'A' }, { label: 'B' }]
    })
  }
}

function row(id: string, toolCalls: ToolCall[], extra: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 1,
    toolCalls,
    ...extra
  }
}

describe('askUserBanner', () => {
  beforeEach(() => {
    resetConversationScopedStoreForTests()
  })

  it('collects pending ask_user from lead messages and scoped rows in order', () => {
    const lead: ChatMessage = row('lead-1', [askUser('tc-1'), askUser('tc-done', 'success')])
    const scoped: ChatMessage = row('scoped-1', [askUser('tc-2')], {
      anchorMessageId: 'lead-1',
      agentInstanceId: 'inst-1'
    })
    expect(pendingAskUserToolCalls([lead, scoped]).map(tc => tc.id)).toEqual(['tc-1', 'tc-2'])
  })

  it('de-duplicates the same tool call id across rows and ignores approvals', () => {
    const dup: ChatMessage = row('scoped-2', [
      askUser('tc-1'),
      { id: 'appr', name: 'terminal', status: 'pending_approval', arguments: '{}' }
    ])
    const lead: ChatMessage = row('lead-1', [askUser('tc-1')])
    expect(pendingAskUserToolCalls([lead, dup]).map(tc => tc.id)).toEqual(['tc-1'])
  })

  it('sees a deep scoped pending ask_user without any frame being mounted', () => {
    const store = useConversationScopedStore()
    const deep = row('scoped-deep', [askUser('tc-deep')], {
      anchorMessageId: 'lead-1',
      agentInstanceId: 'inst-deep',
      spawnDepth: 2
    })
    store.ingestRows('c1', [deep])

    const rows = store.listRows('c1')
    expect(rows.map(r => r.id)).toEqual(['scoped-deep'])
    expect(pendingAskUserToolCalls([...[], ...rows]).map(tc => tc.id)).toEqual(['tc-deep'])
  })

  it('queues the earliest pending question and reports how many remain', () => {
    const view = resolveAskUserBannerView([askUser('tc-1'), askUser('tc-2')], null, 0)
    expect(view?.kind).toBe('ask')
    if (view?.kind !== 'ask') return
    expect(view.toolCall.id).toBe('tc-1')
    expect(view.remaining).toBe(1)
  })

  it('returns null when nothing is pending', () => {
    expect(resolveAskUserBannerView([], null, 0)).toBeNull()
  })

  it('shows the answered linger for 2000ms, then the next pending question', () => {
    const linger = createAskUserLinger({
      toolCallId: 'tc-1',
      selected: ['A'],
      knownPendingIds: ['tc-1', 'tc-2'],
      now: 1_000
    })
    expect(linger.expiresAt).toBe(1_000 + ASK_USER_BANNER_LINGER_MS)

    const during = resolveAskUserBannerView([askUser('tc-2')], linger, 1_000 + 1_999)
    expect(during?.kind).toBe('linger')
    if (during?.kind !== 'linger') return
    expect(during.selected).toEqual(['A'])
    expect(during.remaining).toBe(1)

    const after = resolveAskUserBannerView([askUser('tc-2')], linger, 1_000 + ASK_USER_BANNER_LINGER_MS)
    expect(after?.kind).toBe('ask')
    if (after?.kind !== 'ask') return
    expect(after.toolCall.id).toBe('tc-2')
  })

  it('a brand-new pending question replaces the linger', () => {
    const linger = createAskUserLinger({
      toolCallId: 'tc-1',
      selected: ['A'],
      knownPendingIds: ['tc-1'],
      now: 0
    })
    const view = resolveAskUserBannerView([askUser('tc-new')], linger, 10)
    expect(view?.kind).toBe('ask')
    if (view?.kind !== 'ask') return
    expect(view.toolCall.id).toBe('tc-new')
  })
})
