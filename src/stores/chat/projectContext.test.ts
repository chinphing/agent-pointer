// @vitest-environment happy-dom

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const loadSidebarProjects = vi.hoisted(() => vi.fn())
const deleteProject = vi.hoisted(() => vi.fn())
const saveConversationMeta = vi.hoisted(() => vi.fn())
const sendChat = vi.hoisted(() => vi.fn())
const getPlatformSession = vi.hoisted(() => vi.fn())
const getTaskBoardSnapshot = vi.hoisted(() => vi.fn())

vi.mock('../../lib/api', async importOriginal => {
  const actual = await importOriginal<typeof import('../../lib/api')>()
  return {
    ...actual,
    loadSidebarProjects,
    deleteProject,
    saveConversationMeta,
    sendChat,
    getPlatformSession,
    getTaskBoardSnapshot
  }
})

import { useChatStore } from '../chat'
import type { Project } from '../../types/chat'

const project = (id: string, workspaceRoot: string, isDefault = false): Project => ({
  id,
  name: id,
  workspaceRoot,
  isDefault,
  isPinned: false,
  isArchived: false,
  createdAt: 1,
  updatedAt: 1
})

describe('chat project context flow', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    deleteProject.mockResolvedValue(undefined)
    saveConversationMeta.mockResolvedValue(undefined)
    sendChat.mockResolvedValue(undefined)
    getPlatformSession.mockResolvedValue({ logged_in: true })
    getTaskBoardSnapshot.mockResolvedValue({})
  })

  it('creates conversations in the selected project and inherits its workspace', () => {
    const store = useChatStore()
    store.projects = [project('p1', '/workspace/p1', true)]

    const conversation = store.newConversation('p1')

    expect(conversation.projectId).toBe('p1')
    expect(conversation.workspaceRoot).toBe('/workspace/p1')
  })

  it('creates global conversations without inheriting the active or default project', () => {
    const store = useChatStore()
    const active = project('active', '/workspace/active')
    const fallback = project('default', '/workspace/default', true)
    store.projects = [active, fallback]
    const existing = store.newConversation('active')
    store.openConversation(existing.id)

    const conversation = store.newConversation()

    expect(conversation.projectId).toBeUndefined()
    expect(conversation.workspaceRoot).toBe('')
  })

  it('creates a fresh visible blank for each global new-conversation action', () => {
    const store = useChatStore()

    const first = store.newConversation()
    const second = store.newConversation()

    expect(second.id).not.toBe(first.id)
    expect(store.conversations[0]?.id).toBe(second.id)
    expect(store.currentId).toBe(second.id)
  })

  it('keeps a pre-send project selection pending without binding the conversation', () => {
    const store = useChatStore()
    store.projects = [project('p1', '/workspace/p1'), project('p2', '/workspace/p2')]
    const conversation = store.newConversation()

    expect(store.setConversationProject('p1')).toBe(true)
    expect(conversation.projectId).toBeUndefined()
    expect(conversation.pendingProjectId).toBe('p1')
    expect(conversation.workspaceRoot).toBe('/workspace/p1')
    expect(store.setConversationProject('p2')).toBe(true)
    expect(conversation.projectId).toBeUndefined()
    expect(conversation.pendingProjectId).toBe('p2')

    conversation.messages.push({
      id: 'message-1', role: 'user', content: 'hello', status: 'done', createdAt: 1
    })
    expect(store.setConversationProject('p1')).toBe(false)
    expect(conversation.projectId).toBeUndefined()
    expect(conversation.pendingProjectId).toBe('p2')
  })

  it('binds the pending project before first send and locks later changes', async () => {
    const store = useChatStore()
    const selected = project('selected', '/workspace/selected')
    const fallback = project('default', '/workspace/default', true)
    store.projects = [fallback, selected]
    const conversation = store.newConversation()
    expect(store.setConversationProject(selected.id)).toBe(true)

    await store.sendUserMessage('hello')

    expect(conversation.projectId).toBe(selected.id)
    expect(conversation.pendingProjectId).toBeUndefined()
    expect(conversation.workspaceRoot).toBe(selected.workspaceRoot)
    expect(store.setConversationProject(fallback.id)).toBe(false)
    expect(saveConversationMeta.mock.invocationCallOrder[0]).toBeLessThan(sendChat.mock.invocationCallOrder[0])
    expect(saveConversationMeta.mock.calls[0]?.[0]?.[0]).toMatchObject({
      id: conversation.id,
      projectId: selected.id,
      workspaceRoot: selected.workspaceRoot
    })
  })

  it('binds the default project on first send when no project is pending', async () => {
    const store = useChatStore()
    const fallback = project('default', '/workspace/default', true)
    store.projects = [project('other', '/workspace/other'), fallback]
    const conversation = store.newConversation()

    await store.sendUserMessage('hello')

    expect(conversation.projectId).toBe(fallback.id)
    expect(conversation.pendingProjectId).toBeUndefined()
    expect(conversation.workspaceRoot).toBe(fallback.workspaceRoot)
    expect(store.setConversationProject('other')).toBe(false)
  })

  it('does not append or dispatch a first message when binding persistence fails', async () => {
    const store = useChatStore()
    const selected = project('selected', '/workspace/selected')
    store.projects = [selected]
    const conversation = store.newConversation()
    store.setConversationProject(selected.id)
    saveConversationMeta.mockRejectedValueOnce(new Error('disk unavailable'))

    await store.sendUserMessage('hello')

    expect(conversation.messages).toEqual([])
    expect(conversation.projectId).toBeUndefined()
    expect(conversation.pendingProjectId).toBe(selected.id)
    expect(sendChat).not.toHaveBeenCalled()
  })

  it('removes deleted project conversations and selects the fallback project', async () => {
    const store = useChatStore()
    const deleted = project('deleted', '/workspace/deleted')
    const fallback = project('fallback', '/workspace/fallback', true)
    store.projects = [deleted, fallback]
    const deletedConversation = store.newConversation('deleted', deleted.workspaceRoot)
    const fallbackConversation = store.newConversation('fallback', fallback.workspaceRoot)
    store.openConversation(deletedConversation.id)
    loadSidebarProjects.mockResolvedValue([fallback])

    await store.deleteProject(deleted.id)

    expect(store.conversations.some(c => c.projectId === deleted.id)).toBe(false)
    expect(store.current?.id).toBe(fallbackConversation.id)
    expect(store.current?.projectId).toBe(fallback.id)
    expect(store.current?.workspaceRoot).toBe(fallback.workspaceRoot)
  })
})
