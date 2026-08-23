// @vitest-environment happy-dom

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatMessage } from '../../types/chat'

const loadProject = vi.hoisted(() => vi.fn())
const deleteProject = vi.hoisted(() => vi.fn())
const saveConversationMeta = vi.hoisted(() => vi.fn())
const sendChat = vi.hoisted(() => vi.fn())
const getPlatformSession = vi.hoisted(() => vi.fn())
const getTaskBoardSnapshot = vi.hoisted(() => vi.fn())
const loadConversationMessagesPage = vi.hoisted(() => vi.fn())
const waitForChatStreamReady = vi.hoisted(() => vi.fn())

vi.mock('../../lib/api', async importOriginal => {
  const actual = await importOriginal<typeof import('../../lib/api')>()
  return {
    ...actual,
    loadProject,
    deleteProject,
    saveConversationMeta,
    sendChat,
    getPlatformSession,
    getTaskBoardSnapshot,
    loadConversationMessagesPage,
    waitForChatStreamReady
  }
})

import { useChatStore } from '../chat'

const msg = (id: string, createdAt: number): ChatMessage => ({
  id,
  role: 'user',
  content: id,
  status: 'done',
  createdAt
})

describe('chat loadOlderMessages self-heal', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    loadProject.mockResolvedValue(null)
    deleteProject.mockResolvedValue(undefined)
    saveConversationMeta.mockResolvedValue(undefined)
    sendChat.mockResolvedValue(undefined)
    getPlatformSession.mockResolvedValue({ logged_in: true })
    getTaskBoardSnapshot.mockResolvedValue({})
    waitForChatStreamReady.mockResolvedValue(undefined)
    loadConversationMessagesPage.mockResolvedValue({
      messages: [],
      hasMoreOlder: false,
      hasMoreNewer: false,
      oldestPosition: null,
      newestPosition: null,
      messageCount: 0
    })
  })

  it('rebuilds a missing paging baseline before loading older history', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    // Simulate a long-lived session that was created via newConversation
    // (marked hydrated, never paged) and grew by streaming only.
    conv.messages = [msg('m1', 1), msg('m2', 2)]
    conv.messageCount = 200
    // 方案B: trim fallback（stream 消息尚未写回 position 时）会丢弃建会话
    // 的假分页基线——模拟该状态后自愈必须仍能重建基线。
    const next = { ...store.messagePageByConv }
    delete next[conv.id]
    store.messagePageByConv = next

    loadConversationMessagesPage
      .mockResolvedValueOnce({
        messages: [msg('tail1', 190), msg('tail2', 200)],
        positions: [100, 101],
        hasMoreOlder: true,
        hasMoreNewer: false,
        oldestPosition: 100,
        newestPosition: 101,
        messageCount: 200
      })
      .mockResolvedValueOnce({
        messages: [msg('old1', 50), msg('old2', 60)],
        positions: [50, 60],
        hasMoreOlder: false,
        hasMoreNewer: false,
        oldestPosition: 50,
        newestPosition: 60,
        messageCount: 200
      })

    const added = await store.loadOlderMessages(conv.id)

    expect(added).toBe(true)
    // 1) tail baseline (no beforePosition)  2) older page after the baseline.
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(2)
    expect(loadConversationMessagesPage).toHaveBeenNthCalledWith(1, conv.id, {
      limitTurns: 8
    })
    expect(loadConversationMessagesPage).toHaveBeenNthCalledWith(2, conv.id, {
      limitTurns: 8,
      beforePosition: 100
    })
    expect(conv.messages.map(m => m.id)).toEqual(['old1', 'old2', 'tail1', 'tail2'])
  })

  it('does not fetch again when the rebuilt baseline has no older history', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('m1', 1)]
    // 同上：模拟方案B trim fallback 丢弃假基线后的自愈场景。
    const next = { ...store.messagePageByConv }
    delete next[conv.id]
    store.messagePageByConv = next

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('m1', 1)],
      positions: [5],
      hasMoreOlder: false,
      hasMoreNewer: false,
      oldestPosition: 5,
      newestPosition: 5,
      messageCount: 1
    })

    const added = await store.loadOlderMessages(conv.id)

    expect(added).toBe(false)
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(1)
    expect(loadConversationMessagesPage).toHaveBeenNthCalledWith(1, conv.id, {
      limitTurns: 8
    })
  })

  it('rebuilds when hasMoreOlder is stuck true without a cursor', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('m1', 1), msg('m2', 2)]
    conv.messageCount = 200
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: {
        hasMoreOlder: true,
        hasMoreNewer: false,
        oldestPosition: null,
        newestPosition: null,
        loadingOlder: false,
        loadingNewer: false
      }
    }

    loadConversationMessagesPage
      .mockResolvedValueOnce({
        messages: [msg('tail1', 190), msg('tail2', 200)],
        positions: [100, 101],
        hasMoreOlder: true,
        hasMoreNewer: false,
        oldestPosition: 100,
        newestPosition: 101,
        messageCount: 200
      })
      .mockResolvedValueOnce({
        messages: [msg('old1', 50)],
        positions: [50],
        hasMoreOlder: false,
        hasMoreNewer: false,
        oldestPosition: 50,
        newestPosition: 50,
        messageCount: 200
      })

    const added = await store.loadOlderMessages(conv.id)

    expect(added).toBe(true)
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(2)
    expect(loadConversationMessagesPage).toHaveBeenNthCalledWith(1, conv.id, {
      limitTurns: 8
    })
    expect(loadConversationMessagesPage).toHaveBeenNthCalledWith(2, conv.id, {
      limitTurns: 8,
      beforePosition: 100
    })
    expect(store.messagePageState(conv.id)?.hasMoreOlder).toBe(false)
  })

  it('clears hasMoreOlder when a rebuild still has no cursor', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('m1', 1)]
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: {
        hasMoreOlder: true,
        hasMoreNewer: false,
        oldestPosition: null,
        newestPosition: null,
        loadingOlder: false,
        loadingNewer: false
      }
    }

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('m1', 1)],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: null,
      newestPosition: null,
      messageCount: 1
    })

    const added = await store.loadOlderMessages(conv.id)

    expect(added).toBe(false)
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(1)
    expect(store.messagePageState(conv.id)?.hasMoreOlder).toBe(false)
  })
})
