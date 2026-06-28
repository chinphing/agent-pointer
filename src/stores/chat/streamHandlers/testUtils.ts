import { ref, type Ref } from 'vue'
import { vi } from 'vitest'
import type { ChatMessage, Conversation } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

export function sampleConversation(id = 'conv1'): Conversation {
  return {
    id,
    title: 'Test',
    messages: [],
    createdAt: 0,
    updatedAt: 0,
    skillIds: [],
    toolRoundsUsed: 0,
    toolRoundsUsedSupervisor: 0,
    leadAgentId: 'general',
    agentMode: 'single'
  }
}

export function sampleAssistantMessage(id = 'msg1'): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 0,
    toolCalls: []
  }
}

export function createMockStreamHandlerContext(
  conversations: Conversation[] = [],
  overrides: Partial<StreamHandlerContext> = {}
): StreamHandlerContext {
  const conversationsRef = ref(conversations) as Ref<Conversation[]>
  const noop = () => {}
  const noopAsync = async () => {}

  return {
    conversations: conversationsRef,
    currentId: ref(null),
    computerMonitorPickRequest: ref(null),
    ensureImConversation: noop,
    ensureCronStreamConversation: noop,
    findMessage: (messageId, preferConversationId) => {
      const prefer = preferConversationId?.trim()
      if (prefer) {
        const conv = conversationsRef.value.find(c => c.id === prefer)
        const msg = conv?.messages.find(m => m.id === messageId)
        if (conv && msg) return { conv, msg }
      }
      for (const conv of conversationsRef.value) {
        const msg = conv.messages.find(m => m.id === messageId)
        if (msg) return { conv, msg }
      }
      return null
    },
    persistMeta: vi.fn(),
    markMetaDirty: vi.fn(),
    persistAppend: vi.fn(),
    showUiToast: vi.fn(),
    patchRunState: noop,
    clearRunState: noop,
    clearAllRunStates: noop,
    isConversationGenerating: () => false,
    hasInFlightToolCalls: () => false,
    scheduleMaybeFinishGenerating: noop,
    applyTaskBoardDocument: noop,
    applyTaskBoardDocumentDebounced: noop,
    refreshTaskBoard: noopAsync,
    handleTerminalToolCallStatus: noop,
    syncTerminalLivePopupOutput: noop,
    scheduleDesktopNoticeRemoval: noop,
    applySessionAgentToConversation: noop,
    loadActiveComposerDraft: noop,
    ...overrides
  }
}
