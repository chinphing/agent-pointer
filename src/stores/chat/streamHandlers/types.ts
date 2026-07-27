import type { Ref } from 'vue'
import type {
  AgentMode,
  ChatMessage,
  ComputerMonitorPickRequest,
  Conversation,
  TaskBoardDocument,
  TerminalInputRequest,
  ToolCall
} from '../../../types/chat'

export interface ConversationRunStatePatch {
  generating?: boolean
  activeMessageId?: string | null
}

export interface StreamHandlerContext {
  conversations: Ref<Conversation[]>
  currentId: Ref<string | null>
  computerMonitorPickRequest: Ref<ComputerMonitorPickRequest | null>
  terminalInputRequest: Ref<TerminalInputRequest | null>

  ensureImConversation(conversationId: string, title?: string): void
  ensureCronStreamConversation(conversationId: string): void
  findMessage(
    messageId: string,
    preferConversationId?: string
  ): { conv: Conversation; msg: ChatMessage } | null
  persistMeta(): void
  markMetaDirty(id: string): void
  persistAppend(conversationId: string): void
  showUiToast(message: string, level: 'success' | 'warning' | 'error'): void

  patchRunState(id: string, patch: ConversationRunStatePatch): void
  clearRunState(id: string): void
  clearAllRunStates(): void
  isConversationGenerating(id: string): boolean
  hasInFlightToolCalls(msg: ChatMessage): boolean

  applyTaskBoardDocument(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ): void
  applyTaskBoardDocumentDebounced(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ): void
  refreshTaskBoard(
    conversationId: string,
    taskId?: string,
    anchorMessageId?: string
  ): Promise<void>

  handleTerminalToolCallStatus(
    messageId: string,
    toolCallId: string,
    status: ToolCall['status'],
    traceId?: string,
    scopedMessageId?: string
  ): void
  syncTerminalLivePopupOutput(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ): void
  dismissTerminalLivePopup(): void
  clearTerminalInputRequest(toolCallId: string): void

  scheduleDesktopNoticeRemoval(conversationId: string, messageId: string): void
  applySessionAgentToConversation(
    conv: Conversation,
    leadAgentId: string,
    agentMode: AgentMode
  ): void
  loadActiveComposerDraft(conversationId: string | null): void
  refreshConversationMessages(conversationId: string): void
}
