// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const loadProject = vi.hoisted(() => vi.fn())
const deleteProject = vi.hoisted(() => vi.fn())
const saveConversationMeta = vi.hoisted(() => vi.fn())
const deleteConversationApi = vi.hoisted(() => vi.fn())
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
    deleteConversation: deleteConversationApi,
    sendChat,
    getPlatformSession,
    getTaskBoardSnapshot,
    loadConversationMessagesPage,
    waitForChatStreamReady
  }
})

import { useChatStore } from '../chat'
import { emptyTaskBoardEntry } from './taskBoard'
import {
  clearAllTurnExpandUiState,
  emptyTurnExpandUiState,
  loadTurnExpandUiState,
  resetTurnExpandUiStateForTests,
  saveTurnExpandUiState,
  setTurnExpandStorageScope
} from '../../lib/turnExpandState'

describe('chat deleteConversation cleanup', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    resetTurnExpandUiStateForTests()
    setTurnExpandStorageScope('test-user')
    vi.clearAllMocks()
    loadProject.mockResolvedValue(null)
    deleteProject.mockResolvedValue(undefined)
    saveConversationMeta.mockResolvedValue(undefined)
    deleteConversationApi.mockResolvedValue(undefined)
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

  it('drops the cached task board when a conversation is deleted', async () => {
    const store = useChatStore()
    const keep = store.newConversation()
    const doomed = store.newConversation()
    store.currentId = keep.id

    // Simulate a live task board cached for the doomed conversation.
    store.taskBoards[doomed.id] = emptyTaskBoardEntry()
    expect(store.taskBoards[doomed.id]).toBeDefined()

    await store.deleteConversation(doomed.id)

    expect(store.taskBoards[doomed.id]).toBeUndefined()
    expect(deleteConversationApi).toHaveBeenCalledWith(doomed.id)
    expect(store.conversations.some(c => c.id === doomed.id)).toBe(false)
  })

  it('drops cached turn expand UI when a conversation is deleted', async () => {
    const store = useChatStore()
    const keep = store.newConversation()
    const doomed = store.newConversation()
    store.currentId = keep.id

    const expand = emptyTurnExpandUiState()
    expand.manuallyCollapsedTurnIds.add('turn-x')
    saveTurnExpandUiState(doomed.id, expand)
    expect(loadTurnExpandUiState(doomed.id).manuallyCollapsedTurnIds.has('turn-x')).toBe(true)

    await store.deleteConversation(doomed.id)

    expect(loadTurnExpandUiState(doomed.id).manuallyCollapsedTurnIds.size).toBe(0)
  })
})
