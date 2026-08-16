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
const appendConversationMessages = vi.hoisted(() => vi.fn())

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
    waitForChatStreamReady,
    appendConversationMessages
  }
})

import { useChatStore } from '../chat'

const msg = (id: string, createdAt: number, position?: number): ChatMessage => ({
  id,
  role: 'user',
  content: id,
  status: 'done',
  createdAt,
  ...(position != null ? { position } : {})
})

const asst = (id: string, createdAt: number, position?: number): ChatMessage => ({
  ...msg(id, createdAt, position),
  role: 'assistant'
})

describe('方案B: append positions / trim paging baseline', () => {
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
    appendConversationMessages.mockResolvedValue([])
  })

  it('newConversation seeds a fake paging baseline', () => {
    const store = useChatStore()
    const conv = store.newConversation()
    const state = store.messagePageState(conv.id)
    expect(state).toMatchObject({
      hasMoreOlder: false,
      hasMoreNewer: false,
      oldestPosition: null,
      newestPosition: null,
      loadingOlder: false
    })
  })

  it('persistAppend writes SQLite positions back onto in-memory messages', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    sendChat.mockRejectedValue(new Error('boom'))
    appendConversationMessages.mockImplementation(
      async (_convId: string, messages: ChatMessage[]) =>
        messages.map((m, i) => ({ messageId: m.id, position: 5 + i }))
    )

    await store.sendUserMessage('hello', [])

    // The failed turn pushed an error row; both rows were appended and the
    // returned positions were written back onto the in-memory objects.
    await vi.waitFor(() => {
      expect(conv.messages.length).toBe(2)
      expect(conv.messages.every(m => m.position != null)).toBe(true)
    })
    expect(conv.messages[0]).toMatchObject({ role: 'user', content: 'hello' })
    expect(conv.messages[1].status).toBe('error')
    expect(conv.messages[0].position).toBe(5)
    expect(conv.messages[1].position).toBe(6)
  })

  it('trimConversationHistory advances the paging cursor when rows carry positions', () => {
    const store = useChatStore()
    const conv = store.newConversation()
    // 12 user turns, each with a position, all stale (viewed long ago).
    const messages: ChatMessage[] = []
    for (let i = 1; i <= 12; i += 1) {
      messages.push(msg(`u${i}`, i, i * 10))
      messages.push(asst(`a${i}`, i, i * 10 + 1))
    }
    conv.messages = messages
    for (let i = 1; i <= 12; i += 1) {
      store.markUserMessageViewed(conv.id, `u${i}`, 0)
    }

    const removed = store.trimConversationHistory(conv.id)

    expect(removed).toBeGreaterThan(0)
    expect(conv.messages[0]?.position).not.toBeNull()
    const state = store.messagePageState(conv.id)
    expect(state).toMatchObject({
      oldestPosition: conv.messages[0]?.position,
      hasMoreOlder: true
    })
  })

  it('trimConversationHistory drops the fake baseline when positions are missing', () => {
    const store = useChatStore()
    const conv = store.newConversation()
    // Streamed rows that were trimmed before `append_conversation_messages`
    // returned: no positions on any row.
    const messages: ChatMessage[] = []
    for (let i = 1; i <= 12; i += 1) {
      messages.push(msg(`u${i}`, i))
      messages.push(asst(`a${i}`, i))
    }
    conv.messages = messages
    for (let i = 1; i <= 12; i += 1) {
      store.markUserMessageViewed(conv.id, `u${i}`, 0)
    }
    expect(store.messagePageState(conv.id)).not.toBeNull()

    const removed = store.trimConversationHistory(conv.id)

    expect(removed).toBeGreaterThan(0)
    // Fallback: no authoritative cursor possible → drop the baseline so
    // loadOlderMessages self-heals with a real page.
    expect(store.messagePageState(conv.id)).toBeNull()
  })
})
