// @vitest-environment happy-dom

import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const loadSidebarProjects = vi.hoisted(() => vi.fn())
const deleteProject = vi.hoisted(() => vi.fn())

vi.mock('../../lib/api', async importOriginal => {
  const actual = await importOriginal<typeof import('../../lib/api')>()
  return {
    ...actual,
    loadSidebarProjects,
    deleteProject
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
  })

  it('creates conversations in the selected project and inherits its workspace', () => {
    const store = useChatStore()
    store.projects = [project('p1', '/workspace/p1', true)]

    const conversation = store.newConversation('p1')

    expect(conversation.projectId).toBe('p1')
    expect(conversation.workspaceRoot).toBe('/workspace/p1')
  })

  it('reconciles opened conversation workspace from its project when unset', () => {
    const store = useChatStore()
    store.projects = [project('p1', '/workspace/p1')]
    const conversation = store.newConversation('p1', '')
    conversation.workspaceRoot = ''
    conversation.workspaceUserSet = false
    conversation.workspaceInheritDisabled = false

    expect(store.openConversation(conversation.id)?.id).toBe(conversation.id)
    expect(store.currentId).toBe(conversation.id)
    expect(store.current?.workspaceRoot).toBe('/workspace/p1')
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
