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

function aroundWindowState() {
  return {
    hasMoreOlder: true,
    hasMoreNewer: true,
    oldestPosition: 10,
    newestPosition: 20,
    loadingOlder: false,
    loadingNewer: false
  }
}

async function flushSelectHydrate() {
  await Promise.resolve()
  await Promise.resolve()
}

describe('chat around-window newer paging', () => {
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

  it('appends newer complete turns after the around window', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('u2', 2, 19), asst('a2', 3, 20)]
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('u3', 4), asst('a3', 5)],
      positions: [21, 22],
      hasMoreOlder: true,
      hasMoreNewer: true,
      oldestPosition: 21,
      newestPosition: 22,
      messageCount: 40
    })

    const added = await store.loadNewerMessages(conv.id)

    expect(added).toBe(true)
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(1)
    expect(loadConversationMessagesPage).toHaveBeenCalledWith(conv.id, {
      limitTurns: 8,
      afterPosition: 20
    })
    expect(conv.messages.map(m => m.id)).toEqual(['u2', 'a2', 'u3', 'a3'])
    expect(store.messagePageState(conv.id)).toMatchObject({
      hasMoreOlder: true,
      hasMoreNewer: true,
      oldestPosition: 10,
      newestPosition: 22,
      loadingNewer: false
    })
  })

  it('skips a concurrent newer page while one is in flight', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('u2', 2, 19), asst('a2', 3, 20)]
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    let resolvePage!: (value: unknown) => void
    loadConversationMessagesPage.mockImplementationOnce(
      () => new Promise(resolve => {
        resolvePage = resolve
      })
    )

    const first = store.loadNewerMessages(conv.id)
    const second = await store.loadNewerMessages(conv.id)
    expect(second).toBe(false)
    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(1)

    resolvePage({
      messages: [msg('u3', 4)],
      positions: [21],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: 21,
      newestPosition: 21,
      messageCount: 8
    })
    expect(await first).toBe(true)
  })

  it('re-opens without focus by forcing the tail window', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('mid-u', 1, 19), asst('mid-a', 2, 20)]
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('tail-u', 90), asst('tail-a', 91)],
      positions: [90, 91],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: 90,
      newestPosition: 91,
      messageCount: 40
    })

    store.selectConversation(conv.id)
    await flushSelectHydrate()

    expect(loadConversationMessagesPage).toHaveBeenCalledTimes(1)
    expect(loadConversationMessagesPage).toHaveBeenCalledWith(conv.id, {
      limitTurns: 8
    })
    expect(conv.messages.map(m => m.id)).toEqual(['tail-u', 'tail-a'])
    expect(store.messagePageState(conv.id)?.hasMoreNewer).toBe(false)
  })

  it('does not force the tail when opening a search hit already in memory', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('mid-u', 1, 19), asst('mid-a', 2, 20)]
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    store.selectConversation(conv.id, { focusMessageId: 'mid-u' })
    await flushSelectHydrate()

    expect(loadConversationMessagesPage).not.toHaveBeenCalled()
    expect(conv.messages.map(m => m.id)).toEqual(['mid-u', 'mid-a'])
    expect(store.pendingFocusMessage?.messageId).toBe('mid-u')
  })

  it('clears a stale pending focus when re-opening without a hit', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('mid-u', 1, 19), asst('mid-a', 2, 20)]
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }
    loadConversationMessagesPage.mockResolvedValue({
      messages: [msg('tail-u', 90)],
      positions: [90],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: 90,
      newestPosition: 90,
      messageCount: 8
    })

    store.selectConversation(conv.id, { focusMessageId: 'mid-u' })
    expect(store.pendingFocusMessage?.messageId).toBe('mid-u')

    store.selectConversation(conv.id)
    expect(store.pendingFocusMessage).toBeNull()
    await flushSelectHydrate()
    expect(loadConversationMessagesPage).toHaveBeenCalledWith(conv.id, {
      limitTurns: 8
    })
  })

  it('forces the tail after switching away from an around window and back', async () => {
    const store = useChatStore()
    const around = store.newConversation()
    around.messages = [msg('mid-u', 1, 19), asst('mid-a', 2, 20)]
    around.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [around.id]: aroundWindowState()
    }
    const other = store.newConversation()
    expect(store.currentId).toBe(other.id)

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('tail-u', 90), asst('tail-a', 91)],
      positions: [90, 91],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: 90,
      newestPosition: 91,
      messageCount: 40
    })

    store.selectConversation(around.id)
    await flushSelectHydrate()

    expect(loadConversationMessagesPage).toHaveBeenCalledWith(around.id, {
      limitTurns: 8
    })
    expect(around.messages.map(m => m.id)).toEqual(['tail-u', 'tail-a'])
  })

  it('discards a newer page after the window is replaced with the tail', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('mid-u', 1, 19), asst('mid-a', 2, 20)]
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    let resolveNewer!: (value: unknown) => void
    loadConversationMessagesPage.mockImplementationOnce(
      () => new Promise(resolve => {
        resolveNewer = resolve
      })
    )

    const newerP = store.loadNewerMessages(conv.id)
    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('tail-u', 90), asst('tail-a', 91)],
      positions: [90, 91],
      hasMoreOlder: true,
      hasMoreNewer: false,
      oldestPosition: 90,
      newestPosition: 91,
      messageCount: 40
    })

    expect(await store.ensureMessagesLoaded(conv.id, { force: true })).toBe(true)
    expect(conv.messages.map(m => m.id)).toEqual(['tail-u', 'tail-a'])

    resolveNewer({
      messages: [msg('u3', 4), asst('a3', 5)],
      positions: [21, 22],
      hasMoreOlder: true,
      hasMoreNewer: true,
      oldestPosition: 21,
      newestPosition: 22,
      messageCount: 40
    })
    expect(await newerP).toBe(false)
    expect(conv.messages.map(m => m.id)).toEqual(['tail-u', 'tail-a'])
    expect(store.messagePageState(conv.id)).toMatchObject({
      hasMoreNewer: false,
      newestPosition: 91
    })
  })

  it('does not treat an in-flight around hydrate as the tail', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [msg('other', 1, 1)]
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    let resolveAround!: (value: unknown) => void
    loadConversationMessagesPage
      .mockImplementationOnce(
        () => new Promise(resolve => {
          resolveAround = resolve
        })
      )
      .mockResolvedValueOnce({
        messages: [msg('tail-u', 90), asst('tail-a', 91)],
        positions: [90, 91],
        hasMoreOlder: true,
        hasMoreNewer: false,
        oldestPosition: 90,
        newestPosition: 91,
        messageCount: 40
      })

    const aroundP = store.ensureMessagesAround(conv.id, 'mid-u')
    const forceP = store.ensureMessagesLoaded(conv.id, { force: true })

    resolveAround({
      messages: [msg('mid-u', 10), asst('mid-a', 11)],
      positions: [19, 20],
      hasMoreOlder: true,
      hasMoreNewer: true,
      oldestPosition: 10,
      newestPosition: 20,
      messageCount: 40
    })
    expect(await aroundP).toBe(true)
    expect(await forceP).toBe(true)
    expect(conv.messages.map(m => m.id)).toEqual(['tail-u', 'tail-a'])
    expect(store.messagePageState(conv.id)?.hasMoreNewer).toBe(false)
  })

  it('appends a newer page even when a live generating turn sits at a high position', async () => {
    const store = useChatStore()
    const conv = store.newConversation()
    conv.messages = [
      msg('u2', 2, 19),
      asst('a2', 3, 20),
      asst('live', 90, 90)
    ]
    conv.messages[2]!.status = 'streaming'
    conv.messageCount = 40
    store.messagePageByConv = {
      ...store.messagePageByConv,
      [conv.id]: aroundWindowState()
    }

    loadConversationMessagesPage.mockResolvedValueOnce({
      messages: [msg('u3', 4), asst('a3', 5)],
      positions: [21, 22],
      hasMoreOlder: true,
      hasMoreNewer: true,
      oldestPosition: 21,
      newestPosition: 22,
      messageCount: 40
    })

    const added = await store.loadNewerMessages(conv.id)

    expect(added).toBe(true)
    expect(conv.messages.map(m => m.id)).toEqual(['u2', 'a2', 'u3', 'a3', 'live'])
    expect(store.messagePageState(conv.id)).toMatchObject({
      hasMoreNewer: true,
      newestPosition: 22
    })
  })
})
