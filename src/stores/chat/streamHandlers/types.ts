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

export interface ContextCompressingState {
  scope: string
  messageId?: string
  insertBeforeMessageId?: string
  subAgentId?: string
  subAgentName?: string
  startedAt: number
}

export interface ConversationRunStatePatch {
  generating?: boolean
  activeMessageId?: string | null
  contextCompressing?: ContextCompressingState | null
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
  /** Live check: capture rawContent (debug wire data) only while the raw-content view is enabled. */
  rawContentCaptureEnabled(): boolean
  showUiToast(message: string, level: 'success' | 'warning' | 'error'): void

  patchRunState(id: string, patch: ConversationRunStatePatch): void
  clearRunState(id: string): void
  clearAllRunStates(): void
  isConversationGenerating(id: string): boolean
  setBackgroundJobCount(id: string, count: number): void
  hasBackgroundJobs(id: string): boolean
  /** Clear occupancy when no in-memory background host row is still running. */
  clearBackgroundJobsIfNoneLive(id: string): void
  /**
   * Occupancy is 0: pull persisted host rows if memory still shows「后台执行中」,
   * then only mark true zombies interrupted.
   */
  reconcileBackgroundHostsWhenOccupancyEmpty(id: string): void
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
  /**
   * After force-send / stop interrupt: true when a newer turn is already active.
   * Peek only — Error must not consume the flag, or the trailing Done leaks through.
   */
  isStaleStreamAfterInterrupt(conversationId: string): boolean
  /**
   * After force-send / stop interrupt: true when this Done belongs to the cancelled
   * run while a newer turn is already active (skip timing close + clearRunState).
   */
  consumeStaleDoneAfterInterrupt(conversationId: string): boolean
  /** Mark a finished background conversation as awaiting user view (sidebar solid dot). */
  markConversationAwaitingView(conversationId: string): void
  /** Stamp a user message for in-memory history trim (load / send / viewport). */
  markUserMessageViewed(conversationId: string, messageId: string, at?: number): void
}
