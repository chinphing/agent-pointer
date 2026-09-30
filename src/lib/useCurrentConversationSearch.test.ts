import { nextTick, ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatMessage } from '../types/chat'
import { EMPTY_SEARCH_IDS, EMPTY_SEARCH_MATCHES } from './currentConversationSearch'
import {
  resetConversationScopedStoreForTests,
  useConversationScopedStore
} from './conversationScoped'
import { useCurrentConversationSearch } from './useCurrentConversationSearch'

function leadMessage(id: string, content: string): ChatMessage {
  return { id, role: 'assistant', content, status: 'done', createdAt: 0 }
}

function scopedRow(id: string, content: string): ChatMessage {
  return {
    id,
    role: 'assistant',
    content,
    status: 'streaming',
    createdAt: 0,
    anchorMessageId: 'anchor-1',
    agentInstanceId: 'inst-1'
  }
}

beforeEach(() => {
  resetConversationScopedStoreForTests()
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useCurrentConversationSearch', () => {
  it('keeps the shared empty identity and never reads the store while idle', async () => {
    const store = useConversationScopedStore()
    const row = scopedRow('r1', '中间结论：先看仓库')
    store.ingestRows('c1', [row])
    const liveSignal = vi.spyOn(store, 'getLiveSignal')
    const membership = vi.spyOn(store, 'getMembershipSignal')
    const messages = vi.fn(() => [leadMessage('lead-1', 'pointer 命中')])

    const search = useCurrentConversationSearch({
      query: ref(''),
      conversationId: () => 'c1',
      messages
    })

    expect(search.active.value).toBe(false)
    expect(search.matches.value).toBe(EMPTY_SEARCH_MATCHES)
    expect(search.matchMessageIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchToolCallIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchContentIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(messages).not.toHaveBeenCalled()
    expect(liveSignal).not.toHaveBeenCalled()
    expect(membership).not.toHaveBeenCalled()

    // A streamed chunk lands in the scoped store (the live signal really moves) …
    const liveBefore = store.getLiveSignal('c1', 'inst-1')
    row.content = '中间结论：先看仓库，然后改 store'
    store.touchRow('c1', 'r1')
    expect(store.getLiveSignal('c1', 'inst-1')).not.toBe(liveBefore)

    // … and the idle search neither subscribes to it nor hands out new array identities.
    liveSignal.mockClear()
    membership.mockClear()
    await nextTick()
    expect(search.matches.value).toBe(EMPTY_SEARCH_MATCHES)
    expect(search.matchMessageIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchToolCallIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchContentIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(liveSignal).not.toHaveBeenCalled()
    expect(membership).not.toHaveBeenCalled()
  })

  it('treats a whitespace-only query as idle', () => {
    const store = useConversationScopedStore()
    store.ingestRows('c1', [scopedRow('r1', 'anything')])
    const liveSignal = vi.spyOn(store, 'getLiveSignal')

    const search = useCurrentConversationSearch({
      query: ref('   '),
      conversationId: () => 'c1',
      messages: () => [leadMessage('lead-1', 'anything')]
    })

    expect(search.active.value).toBe(false)
    expect(search.matchMessageIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matches.value).toBe(EMPTY_SEARCH_MATCHES)
    expect(liveSignal).not.toHaveBeenCalled()
  })

  it('searches transcript and scoped rows while a query is active', async () => {
    const store = useConversationScopedStore()
    const row = scopedRow('r1', '中间结论：先看仓库')
    store.ingestRows('c1', [row])

    const search = useCurrentConversationSearch({
      query: ref('中间结论'),
      conversationId: () => 'c1',
      messages: () => [leadMessage('lead-1', '中间结论：先看仓库')]
    })

    expect(search.active.value).toBe(true)
    expect(search.matches.value).toEqual([
      { messageId: 'lead-1' },
      { messageId: 'anchor-1', contentMessageId: 'r1' }
    ])
    expect(search.matchMessageIds.value).toEqual(['lead-1', 'anchor-1'])
    expect(search.matchContentIds.value).toEqual(['r1'])

    // Scoped rows keep streaming while the query is active — results must follow.
    row.content = '换成别的结论'
    store.touchRow('c1', 'r1')
    await nextTick()
    expect(search.matches.value).toEqual([{ messageId: 'lead-1' }])
    expect(search.matchMessageIds.value).toEqual(['lead-1'])
    expect(search.matchContentIds.value).toBe(EMPTY_SEARCH_IDS)
  })

  it('falls back to the shared empty ids when an active query has no hits', async () => {
    const store = useConversationScopedStore()
    store.ingestRows('c1', [scopedRow('r1', 'nothing here')])

    const query = ref('pointer')
    const search = useCurrentConversationSearch({
      query,
      conversationId: () => 'c1',
      messages: () => []
    })

    expect(search.active.value).toBe(true)
    expect(search.matches.value).toBe(EMPTY_SEARCH_MATCHES)
    expect(search.matchMessageIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchToolCallIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchContentIds.value).toBe(EMPTY_SEARCH_IDS)

    // Switching between two hit-less queries must not hand out fresh array identities.
    query.value = 'also nothing'
    await nextTick()
    expect(search.matches.value).toBe(EMPTY_SEARCH_MATCHES)
    expect(search.matchMessageIds.value).toBe(EMPTY_SEARCH_IDS)
    expect(search.matchContentIds.value).toBe(EMPTY_SEARCH_IDS)
  })
})
