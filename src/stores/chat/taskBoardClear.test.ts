// @vitest-environment happy-dom
import { ref, type Ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { TaskBoardDocument } from '../../types/chat'
import {
  createTaskBoardManager,
  type ConversationTaskBoardState
} from './taskBoard'

function doc(goal: string): TaskBoardDocument {
  return {
    version: 2,
    task_id: 'tb',
    meta: { goal, status: 'active' },
    board: []
  }
}

function makeManager(taskBoards: Ref<Record<string, ConversationTaskBoardState>>) {
  return createTaskBoardManager({
    taskBoards,
    getMessages: () => [],
    fetchSnapshot: vi.fn(),
    showChildBoards: () => false
  })
}

describe('taskBoardManager.clearConversation', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('removes the cached board entry', () => {
    const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
    const mgr = makeManager(taskBoards)
    mgr.applyTaskBoardDocument('conv1', 'conv1', doc('goal'))
    expect(taskBoards.value.conv1).toBeDefined()

    mgr.clearConversation('conv1')
    expect(taskBoards.value.conv1).toBeUndefined()
  })

  it('cancels pending debounced writes so they do not recreate the entry', () => {
    const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
    const mgr = makeManager(taskBoards)
    mgr.applyTaskBoardDocumentDebounced('conv1', 'conv1', doc('goal'))
    mgr.clearConversation('conv1')

    vi.advanceTimersByTime(1000)
    expect(taskBoards.value.conv1).toBeUndefined()
  })

  it('keeps other conversations untouched', () => {
    const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
    const mgr = makeManager(taskBoards)
    mgr.applyTaskBoardDocument('conv1', 'conv1', doc('a'))
    mgr.applyTaskBoardDocument('conv2', 'conv2', doc('b'))

    mgr.clearConversation('conv1')
    expect(taskBoards.value.conv1).toBeUndefined()
    expect(taskBoards.value.conv2).toBeDefined()
  })
})
