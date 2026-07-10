import { defineStore } from 'pinia'
import { ref, computed, watch, nextTick } from 'vue'
import {
  sendChat, cancelChat, abortTerminalCommand, approveToolCall, onStream,
  loadConversationMetas,
  loadConversationMessages,
  saveConversationMeta,
  deleteConversation as deleteConversationApi,
  appendConversationMessages,
  saveChatAttachment
} from '../lib/api'
import type {
  AgentMode,
  ChatMessage,
  ComputerMonitorPickRequest,
  Conversation,
  ConversationCursor,
  ConversationMeta,
  ConversationMetaPage,
  OutboundQueueItem,
  StreamEvent,
  TerminalInputRequest,
  ToolCall,
  TaskBoardDocument
} from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { CODER_AGENT_ID, CODER_DEFAULT_SKILL_IDS, GENERAL_AGENT_ID } from '../lib/agentUi'
import { getTaskBoardSnapshot } from '../lib/api'
import { subTaskIdFromTraceId } from '../lib/subAgentStats'
import { resolveStreamWriteMessage, rehydrateAgentTracesFromScopedMessages, scopedMessagesForTrace } from '../lib/subAgentMessages'
import { stripWireAttachmentFields } from '../lib/messageNormalizer'
import { messagesForChatDispatch } from '../lib/chatDispatchHistory'
import {
  clearReasoningDeltaBuffer,
  flushReasoningDeltaBuffer,
  setReasoningDeltaApplyHandler
} from '../lib/reasoningDeltaBatch'
import { imConversationTitle, isImConversation } from '../lib/channel-labels'
import {
  DEFAULT_CONVERSATION_TITLE,
  maybeUpdateConversationTitle
} from '../lib/conversationTitle'
import { dedupeImInboundUserMessages } from '../lib/imMessageDedupe'
import {
  getComposerAttachmentContentBase64,
  getComposerAttachmentDataUrl,
  hydrateComposerAttachments,
  releaseComposerAttachment
} from '../lib/attachmentPayloadStore'
import type { ComposerAttachment, ComposerDraft } from '../types/chat'
import {
  createTaskBoardManager,
  type ConversationTaskBoardState
} from './chat/taskBoard'
import {
  createTerminalLiveManager,
  type TerminalLivePopup
} from './chat/terminalLive'
import { createDesktopNoticeScheduler } from './chat/desktopNotice'

export type { ConversationTaskBoardState, TerminalLivePopup }
import {
  assistantHasDeliverableContent,
  isDiscardableEmptyAssistant,
  isEphemeralDesktopNoticeMessage
} from '../lib/assistantMessageKind'
import {
  ensureSubTrace,
  migrateLegacyTraceUiState
} from '../lib/subAgentSession'
import { useSkillsStore } from './skills'
import { useSettingsStore } from './settings'
import { usePlatformAuthStore } from './platformAuth'
import { isTauriRuntime } from '../lib/runtime'
import { dispatchStreamEvent, type StreamHandlerContext } from './chat/streamHandlers/dispatch'
import {
  assistantTurnActivelyRunning,
  hasInFlightToolCalls,
  normalizeInterruptedAssistantStatuses,
  normalizeStaleEndedAssistantTurn,
  removeAssistantMessage,
  uid
} from './chat/helpers'

function stripEphemeralDesktopNoticesForDisk(conversations: Conversation[]): Conversation[] {
  return conversations.map(c => ({
    ...c,
    messages: stripWireAttachmentFields(
      c.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
    )
  }))
}

/** Desktop shell with default title and no messages (duplicate-prone if we always insert new rows). */
function isBlankDesktopConversation(conv: Conversation): boolean {
  if (isImConversation(conv.id)) return false
  if (conv.title !== DEFAULT_CONVERSATION_TITLE) return false
  // Meta-only boot path: messageCount is set from the DB row. Prefer it so we
  // can detect blanks without hydrating messages.
  if (typeof conv.messageCount === 'number') return conv.messageCount === 0
  const visible = conv.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
  return visible.length === 0
}

function pruneDuplicateBlankConversations(list: Conversation[]): {
  list: Conversation[]
  changed: boolean
  prunedBlankIds: string[]
} {
  const blanks = list.filter(isBlankDesktopConversation)
  if (blanks.length <= 1) return { list, changed: false, prunedBlankIds: [] }
  const keepId = blanks[0]!.id
  const prunedBlankIds = blanks.filter(b => b.id !== keepId).map(b => b.id)
  return {
    list: list.filter(c => !isBlankDesktopConversation(c) || c.id === keepId),
    changed: true,
    prunedBlankIds
  }
}

function normalizeSubAgentTraces(conversations: Conversation[]) {
  for (const conv of conversations) {
    rehydrateAgentTracesFromScopedMessages(conv)
    for (const msg of conv.messages) {
      for (const trace of msg.agentTrace ?? []) {
        if ((trace.depth ?? 0) === 0) continue
        migrateLegacyTraceUiState(trace)
        const terminal = trace.status === 'completed' || trace.status === 'failed'
        if (terminal && !trace.userExpanded) {
          trace.collapsed = true
        }
      }
    }
  }
}

function removeDiscardableAssistant(conv: Conversation, messageId: string | null | undefined): boolean {
  if (!messageId) return false
  const msg = conv.messages.find(m => m.id === messageId)
  if (!msg || !isDiscardableEmptyAssistant(msg)) return false
  return removeAssistantMessage(conv, messageId)
}

function isPlatformLoginErrorMessage(msg: ChatMessage): boolean {
  if (msg.role !== 'assistant' || msg.status !== 'error') return false
  const text = msg.errorMessage ?? ''
  return (
    text.includes('登录已失效') ||
    text.includes('请先登录 Pointer 账户') ||
    text.includes('平台登录态刷新失败')
  )
}

interface ConversationRunState {
  generating: boolean
  activeMessageId: string | null
}

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  const currentId = ref<string | null>(null)
  const runByConversation = ref<Record<string, ConversationRunState>>({})

  /** FIFO outbound sends waiting while the session turn is still running (Hermes-style). */
  const outboundQueues = ref<Record<string, OutboundQueueItem[]>>({})

  function outboundQueueItems(conversationId: string): OutboundQueueItem[] {
    const key = conversationId.trim()
    if (!key) return []
    return outboundQueues.value[key] ?? []
  }

  function outboundQueueCount(conversationId: string): number {
    return outboundQueueItems(conversationId).length
  }

  function enqueueOutbound(conversationId: string, item: OutboundQueueItem) {
    const key = conversationId.trim()
    if (!key) return
    const prev = outboundQueues.value[key] ?? []
    outboundQueues.value = { ...outboundQueues.value, [key]: [...prev, item] }
  }

  function dequeueOutbound(conversationId: string): OutboundQueueItem | null {
    const key = conversationId.trim()
    const queue = outboundQueues.value[key]
    if (!queue?.length) return null
    const [head, ...rest] = queue
    if (rest.length) {
      outboundQueues.value = { ...outboundQueues.value, [key]: rest }
    } else {
      const next = { ...outboundQueues.value }
      delete next[key]
      outboundQueues.value = next
    }
    return head ?? null
  }

  function removeOutboundQueueItem(conversationId: string, itemId: string) {
    const key = conversationId.trim()
    const id = itemId.trim()
    const queue = outboundQueues.value[key]
    if (!queue?.length) return
    const next = queue.filter(item => item.id !== id)
    if (next.length === queue.length) return
    if (next.length) {
      outboundQueues.value = { ...outboundQueues.value, [key]: next }
    } else {
      const map = { ...outboundQueues.value }
      delete map[key]
      outboundQueues.value = map
    }
  }

  function clearOutboundQueue(conversationId: string) {
    const key = conversationId.trim()
    if (!key || !outboundQueues.value[key]) return
    const next = { ...outboundQueues.value }
    delete next[key]
    outboundQueues.value = next
  }
  /** Ephemeral banner (e.g. computer screenshot done); not persisted. */
  const uiToast = ref<{ message: string; level: 'success' | 'warning' | 'error' } | null>(null)
  const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
  const taskBoardMgr = createTaskBoardManager({
    taskBoards,
    getMessages: (id) => conversations.value.find(c => c.id === id)?.messages,
    fetchSnapshot: getTaskBoardSnapshot,
    showChildBoards: () => useSettingsStore().settings.taskBoardShowChildBoards === true
  })
  const desktopNotice = createDesktopNoticeScheduler({ conversations })
  /** One-shot composer draft from home experience suggestions. */
  const composerPrefill = ref<string | null>(null)
  /** In-memory composer drafts per conversation (survives layout / compact-mode remounts). */
  const composerDraftByConvId = ref<Record<string, ComposerDraft>>({})
  /** Active composer state (shared across inline/footer instances and compact-mode remounts). */
  const composerText = ref('')
  const composerAttachments = ref<ComposerAttachment[]>([])
  const composerDraftHydrating = ref(false)
  /** Set when a computer sub-agent needs monitor selection before it can start. */
  const computerMonitorPickRequest = ref<ComputerMonitorPickRequest | null>(null)
  const terminalInputRequest = ref<TerminalInputRequest | null>(null)
  const terminalLivePopup = ref<TerminalLivePopup | null>(null)
  let uiToastTimer: ReturnType<typeof setTimeout> | null = null
  let unlisten: (() => void) | null = null
  let saveTimer: number | null = null
  let composerDraftTimer: number | null = null
  /** Conversation ids whose meta changed and need persisting on next flush. */
  const dirtyMetaIds = ref(new Set<string>())

  /** Cursor pagination page size for the sidebar meta list. */
  const META_PAGE_SIZE = 50
  /** Cursor for the next page of conversation metas (null = no more / first page). */
  const nextCursor = ref<ConversationCursor | null>(null)
  /** True while a `loadMoreConversations` fetch is in flight (UI spinner guard). */
  const loadingMoreConversations = ref(false)
  /** Conversation ids whose messages have been loaded into memory this session. */
  const hydratedIds = ref<Set<string>>(new Set())
  /** Conversation ids currently fetching messages from disk. */
  const messagesLoadingIds = ref<Set<string>>(new Set())

  const current = computed(() =>
    conversations.value.find(c => c.id === currentId.value) || null
  )

  const currentOutboundQueue = computed(() => {
    const id = currentId.value?.trim()
    if (!id) return [] as OutboundQueueItem[]
    return outboundQueues.value[id] ?? []
  })

  function conversationNeedsMessageHydration(conv: Conversation | null | undefined): boolean {
    if (!conv) return false
    if (messagesLoadingIds.value.has(conv.id)) return true
    if ((conv.messageCount ?? 0) > 0 && conv.messages.length === 0) return true
    return !hydratedIds.value.has(conv.id) && conv.messages.length === 0
  }

  const isCurrentConversationHydrating = computed(() =>
    conversationNeedsMessageHydration(current.value)
  )

  /** True when there are more conversation metas to fetch from the DB. */
  const hasMoreConversations = computed(() => nextCursor.value !== null)

  function runStateFor(id: string): ConversationRunState {
    return runByConversation.value[id] ?? { generating: false, activeMessageId: null }
  }

  function patchRunState(id: string, patch: Partial<ConversationRunState>) {
    runByConversation.value = {
      ...runByConversation.value,
      [id]: { ...runStateFor(id), ...patch }
    }
  }

  function clearRunState(id: string) {
    const key = id.trim()
    if (!key) return
    cancelGeneratingClearTimer(key)
    patchRunState(key, { generating: false, activeMessageId: null })
    queueMicrotask(() => {
      void drainOutboundQueue(key)
    })
  }

  let drainingOutbound = new Set<string>()

  async function drainOutboundQueue(conversationId: string) {
    const convId = conversationId.trim()
    if (!convId || isConversationGenerating(convId) || drainingOutbound.has(convId)) return
    const item = dequeueOutbound(convId)
    if (!item) return

    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      await drainOutboundQueue(convId)
      return
    }

    drainingOutbound.add(convId)
    try {
      const userMsg: ChatMessage = {
        id: item.id,
        role: 'user',
        content: item.content,
        status: 'done',
        createdAt: item.createdAt,
        ...(item.attachments?.length ? { attachments: item.attachments } : {})
      }
      conv.messages.push(userMsg)
      maybeUpdateConversationTitle(conv)
      conv.updatedAt = Date.now()
      await dispatchChatTurn(conv)
    } finally {
      drainingOutbound.delete(convId)
    }
  }

  async function dispatchChatTurn(conv: Conversation) {
    patchRunState(conv.id, { generating: true, activeMessageId: null })
    await flushPersistMeta()
    await refreshTaskBoard(conv.id)

    await sendChat({
      conversationId: conv.id,
      messages: messagesForChatDispatch(conv.messages),
      enabledSkillIds: enabledSkillIdsForRequest(conv),
      agentMode: effectiveConversationAgentMode(conv),
      leadAgentId: effectiveConversationLeadAgentId(conv),
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: conv.workspaceInheritDisabled ? '' : (conv.workspaceRoot ?? '').trim(),
      ...(conv.workspaceInheritDisabled ? { workspaceInheritDisabled: true } : {})
    }).catch(err => {
      clearRunState(conv.id)
      console.error('sendChat error', err)
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage: String(err)
      })
      persistAppend(conv.id)
    })
  }

  const generatingClearTimers = new Map<string, ReturnType<typeof setTimeout>>()

  function cancelGeneratingClearTimer(conversationId: string) {
    const key = conversationId.trim()
    if (!key) return
    const timer = generatingClearTimers.get(key)
    if (timer != null) {
      clearTimeout(timer)
      generatingClearTimers.delete(key)
    }
  }

  function maybeFinishGenerating(conversationId: string, messageId: string) {
    const convId = conversationId.trim()
    const msgId = messageId.trim()
    if (!convId || !msgId) return
    if (!isConversationGenerating(convId)) return

    const activeId = runStateFor(convId).activeMessageId
    if (activeId && activeId !== msgId) return

    const conv = conversations.value.find(c => c.id === convId)
    const msg = conv?.messages.find(m => m.id === msgId)
    if (!msg || msg.role !== 'assistant') return
    if (!assistantHasDeliverableContent(msg)) return
    if (hasInFlightToolCalls(msg)) return

    if (!activeId) {
      const streaming = conv!.messages.filter(
        m => m.role === 'assistant' && (m.status === 'streaming' || m.status === 'pending')
      )
      if (streaming.length !== 1 || streaming[0]!.id !== msgId) return
    }

    console.warn(
      '[chat] finishing generating after assistant reply (done event missing?) conv=%s msg=%s',
      convId,
      msgId
    )
    clearRunState(convId)
    msg.status = 'done'
    msg.contentStreaming = false
  }

  function scheduleMaybeFinishGenerating(conversationId: string, messageId: string) {
    const convId = conversationId.trim()
    const msgId = messageId.trim()
    if (!convId || !msgId) return
    cancelGeneratingClearTimer(convId)
    generatingClearTimers.set(
      convId,
      window.setTimeout(() => {
        generatingClearTimers.delete(convId)
        maybeFinishGenerating(convId, msgId)
      }, 400)
    )
  }

  function clearAllRunStates() {
    runByConversation.value = {}
  }

  function isConversationGenerating(id: string): boolean {
    return runStateFor(id).generating
  }

  /** Restore run UI when switching back to a conversation still streaming in the background. */
  function reconcileRunStateForConversation(conversationId: string) {
    const convId = conversationId.trim()
    if (!convId || isConversationGenerating(convId)) return
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) return
    for (let i = conv.messages.length - 1; i >= 0; i--) {
      const msg = conv.messages[i]
      if (msg.role !== 'assistant') continue
      normalizeStaleEndedAssistantTurn(msg)
      if (assistantTurnActivelyRunning(msg)) {
        patchRunState(convId, { generating: true, activeMessageId: msg.id })
        return
      }
      break
    }
  }

  const generating = computed(() => {
    const id = currentId.value
    return id ? isConversationGenerating(id) : false
  })

  const activeGeneratingMessageId = computed(() => {
    const id = currentId.value
    return id ? runStateFor(id).activeMessageId : null
  })

  function effectiveConversationAgentMode(conv?: Conversation | null): AgentMode {
    const mode = conv?.agentMode?.trim()
    if (mode === 'supervisor' || mode === 'single') return mode
    return 'single'
  }

  function effectiveConversationLeadAgentId(conv?: Conversation | null): string {
    const id = conv?.leadAgentId?.trim()
    return id || DEFAULT_LEAD_AGENT_ID
  }

  function applySessionAgentToConversation(
    conv: Conversation,
    leadAgentId: string,
    agentMode: AgentMode
  ) {
    conv.leadAgentId = leadAgentId.trim() || DEFAULT_LEAD_AGENT_ID
    conv.agentMode = agentMode
    conv.updatedAt = Date.now()
  }

  function enabledSkillIdsForRequest(conv: Conversation): string[] {
    if (effectiveConversationAgentMode(conv) === 'supervisor') return []
    const lead = effectiveConversationLeadAgentId(conv)
    if (lead === GENERAL_AGENT_ID) return [...useSkillsStore().enabledIds]
    if (lead === CODER_AGENT_ID) return [...CODER_DEFAULT_SKILL_IDS]
    return []
  }

  async function refreshSubAgentTaskBoards(conversationId: string) {
    const conv = conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    const jobs: Promise<void>[] = []
    for (const msg of conv.messages) {
      for (const trace of msg.agentTrace ?? []) {
        if ((trace.depth ?? 0) === 0) continue
        const taskId = subTaskIdFromTraceId(trace.id)
        if (!taskId) continue
        jobs.push(refreshTaskBoard(conversationId, taskId, trace.id))
      }
    }
    await Promise.all(jobs)
  }

  /** Child task board for a delegated trace (`{taskId}:{agentId}`). */
  function lookupChildTaskBoard(
    convId: string | null,
    traceId: string,
    legacyLeadMessageId?: string | null
  ): TaskBoardDocument | null {
    return taskBoardMgr.lookupChildTaskBoard(convId, traceId, legacyLeadMessageId)
  }

  function ensureImConversation(conversationId: string, title?: string) {
    if (!isImConversation(conversationId)) return
    if (conversations.value.some(c => c.id === conversationId)) return
    conversations.value = [
      {
        id: conversationId,
        title: title ?? imConversationTitle(conversationId),
        messages: [],
        createdAt: Date.now(),
        updatedAt: Date.now(),
        skillIds: [],
        toolRoundsUsed: 0,
        toolRoundsUsedSupervisor: 0,
        leadAgentId: DEFAULT_LEAD_AGENT_ID,
        agentMode: 'single'
      },
      ...conversations.value
    ]
    markMetaDirty(conversationId)
  }

  /** Ensure an automation session shell exists when stream events target the active view. */
  function ensureCronStreamConversation(conversationId: string) {
    if (!conversationId.startsWith('cron:') && !conversationId.startsWith('webhook:')) return
    if (currentId.value !== conversationId) return
    if (conversations.value.some(c => c.id === conversationId)) return
    const title = conversationId.startsWith('webhook:')
      ? `[Webhook] ${conversationId.slice('webhook:'.length)}`
      : '[定时] cron 会话'
    conversations.value.unshift({
      id: conversationId,
      title,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: '',
      workspaceUserSet: false,
      workspaceInheritDisabled: false,
      leadAgentId: DEFAULT_LEAD_AGENT_ID,
      agentMode: 'single'
    })
  }

  function metaToConversationShell(m: ConversationMeta): Conversation {
    return {
      id: m.id,
      title: m.title,
      createdAt: m.createdAt,
      updatedAt: m.updatedAt,
      messages: [],
      skillIds: m.skillIds ?? [],
      toolRoundsUsed: m.toolRoundsUsed,
      toolRoundsUsedSupervisor: m.toolRoundsUsedSupervisor,
      computerMonitorId: m.computerMonitorId,
      workspaceRoot: m.workspaceRoot,
      workspaceUserSet: m.workspaceUserSet,
      workspaceInheritDisabled: m.workspaceInheritDisabled,
      leadAgentId: m.leadAgentId,
      agentMode: m.agentMode,
      messageCount: m.messageCount
    }
  }

  async function init() {
    // Boot path: load only the first page of conversation metas (no messages).
    // The active conversation's messages are hydrated on demand below; other
    // conversations are hydrated when the user selects them.
    const firstPage = await loadConversationMetas(null, META_PAGE_SIZE).catch(err => {
      console.error('[chat] loadConversationMetas failed at boot', err)
      return { items: [] as ConversationMeta[], nextCursor: null } as ConversationMetaPage
    })
    const metas = firstPage.items
    const pruned = pruneDuplicateBlankConversations(metas.map(metaToConversationShell))
    const shells = pruned.list
    for (const conv of shells) {
      if (!conv.leadAgentId?.trim()) conv.leadAgentId = DEFAULT_LEAD_AGENT_ID
      if (!conv.agentMode?.trim()) conv.agentMode = 'single'
    }
    conversations.value = shells
    nextCursor.value = firstPage.nextCursor
    // IM conversations can derive a title purely from their id (no messages
    // needed); persist any such title fixes as targeted delta writes. Desktop
    // blank pruning needs no upsert here — pruned blanks are deleted explicitly
    // below via deleteConversationApi, and retained conversations are unchanged.
    for (const conv of shells) {
      if (maybeUpdateConversationTitle(conv)) markMetaDirty(conv.id)
    }
    // Explicitly delete any duplicate blank conversations pruned above (only
    // those we actually saw this boot).
    for (const blankId of pruned.prunedBlankIds) {
      void deleteConversationApi(blankId).catch(err =>
        console.error('[chat] boot prune: deleteConversation failed', blankId, err)
      )
    }

    if (shells.length === 0) {
      newConversation()
    } else {
      currentId.value = shells[0]!.id
      await ensureMessagesLoaded(shells[0]!.id)
      loadActiveComposerDraft(currentId.value)
    }

    if (!unlisten) unlisten = await onStream(handleEvent)
    if (currentId.value) {
      void refreshTaskBoard(currentId.value)
      void refreshSubAgentTaskBoards(currentId.value)
    }
  }

  /**
   * Load messages for a conversation if not already hydrated this session.
   * Runs the per-conversation normalizers that previously ran once over every
   * conversation at boot. Skips conversations currently generating unless
   * `force` is set (automation "查看会话" while a webhook/cron run is active).
   */
  function mergeHydratedMessages(inMemory: ChatMessage[], fromDb: ChatMessage[]): ChatMessage[] {
    if (inMemory.length === 0) return fromDb
    const dbById = new Map(fromDb.map(m => [m.id, m]))
    const merged: ChatMessage[] = []
    for (const dbMsg of fromDb) {
      const live = inMemory.find(m => m.id === dbMsg.id)
      if (
        live
        && (live.status === 'streaming'
          || live.status === 'pending'
          || live.contentStreaming)
      ) {
        merged.push({
          ...dbMsg,
          ...live,
          toolCalls: live.toolCalls?.length ? live.toolCalls : dbMsg.toolCalls
        })
      } else {
        merged.push(dbMsg)
      }
    }
    for (const live of inMemory) {
      if (!dbById.has(live.id)) merged.push(live)
    }
    return merged
  }

  async function ensureMessagesLoaded(id: string, options?: { force?: boolean }): Promise<void> {
    const convId = id.trim()
    if (!convId) return
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      console.warn('[chat] ensureMessagesLoaded: missing conversation', convId)
      return
    }
    const staleHydration =
      hydratedIds.value.has(convId)
      && (conv.messageCount ?? 0) > 0
      && conv.messages.length === 0
    if (!options?.force && hydratedIds.value.has(convId) && !staleHydration) return
    if (staleHydration) {
      console.warn('[chat] ensureMessagesLoaded: stale hydration, reloading', convId)
    }
    if (!options?.force && !staleHydration && isConversationGenerating(convId)) {
      console.info('[chat] ensureMessagesLoaded: skip hydrating generating conversation', convId)
      return
    }
    if (messagesLoadingIds.value.has(convId)) return
    messagesLoadingIds.value = new Set([...messagesLoadingIds.value, convId])
    try {
      const messages = await loadConversationMessages(convId)
      const stripped = stripWireAttachmentFields(
        messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
      )
      let next = stripped
      if (isImConversation(convId)) {
        next = dedupeImInboundUserMessages(convId, stripped)
      }
      if (isConversationGenerating(convId) && conv.messages.length > 0) {
        next = mergeHydratedMessages(conv.messages, next)
        console.info(
          '[chat] ensureMessagesLoaded: merged DB rows with in-memory stream',
          convId,
          next.length
        )
      }
      conv.messages = next
      if (!isConversationGenerating(convId)) {
        normalizeInterruptedAssistantStatuses([conv])
      }
      normalizeSubAgentTraces([conv])
      hydratedIds.value.add(convId)
      console.info('[chat] ensureMessagesLoaded: hydrated', convId, next.length)
      reconcileRunStateForConversation(convId)
    } catch (err) {
      console.error('[chat] ensureMessagesLoaded: load messages failed', convId, err)
    } finally {
      const next = new Set(messagesLoadingIds.value)
      next.delete(convId)
      messagesLoadingIds.value = next
    }
  }

  /** Fetch the next page of conversation metas and append to the sidebar. */
  async function loadMoreConversations(): Promise<void> {
    if (!nextCursor.value || loadingMoreConversations.value) return
    loadingMoreConversations.value = true
    try {
      const page = await loadConversationMetas(nextCursor.value, META_PAGE_SIZE)
      const existingIds = new Set(conversations.value.map(c => c.id))
      const fresh = page.items
        .map(metaToConversationShell)
        .filter(c => !existingIds.has(c.id))
      if (fresh.length > 0) {
        for (const conv of fresh) {
          if (!conv.leadAgentId?.trim()) conv.leadAgentId = DEFAULT_LEAD_AGENT_ID
          if (!conv.agentMode?.trim()) conv.agentMode = 'single'
        }
        conversations.value = [...conversations.value, ...fresh]
      }
      nextCursor.value = page.nextCursor
      console.info('[chat] loadMoreConversations: appended', fresh.length, 'nextCursor=', page.nextCursor)
    } catch (err) {
      console.error('[chat] loadMoreConversations failed', err)
    } finally {
      loadingMoreConversations.value = false
    }
  }

  function toConversationMeta(c: Conversation): ConversationMeta {
    return {
      id: c.id,
      title: c.title,
      createdAt: c.createdAt,
      updatedAt: c.updatedAt,
      skillIds: c.skillIds,
      toolRoundsUsed: c.toolRoundsUsed,
      toolRoundsUsedSupervisor: c.toolRoundsUsedSupervisor,
      computerMonitorId: c.computerMonitorId,
      workspaceRoot: c.workspaceRoot,
      workspaceUserSet: c.workspaceUserSet,
      workspaceInheritDisabled: c.workspaceInheritDisabled,
      leadAgentId: c.leadAgentId,
      agentMode: c.agentMode
    }
  }

  function persistMeta() {
    // Mark ALL loaded conversations dirty and schedule a flush. Use this only
    // for rare multi-conversation changes (e.g. boot pruning). The hot path
    // (stream handlers, single-conversation edits) uses markMetaDirty(id) so
    // we don't upsert every loaded meta on every chat event.
    for (const c of conversations.value) dirtyMetaIds.value.add(c.id)
    scheduleMetaFlush()
  }

  function markMetaDirty(id: string) {
    dirtyMetaIds.value.add(id)
    scheduleMetaFlush()
  }

  function scheduleMetaFlush() {
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      void flushPersistMeta()
    }, 400)
  }

  async function flushPersistMeta() {
    if (saveTimer) {
      window.clearTimeout(saveTimer)
      saveTimer = null
    }
    const ids = dirtyMetaIds.value
    if (ids.size === 0) return
    // Snapshot+clear before the await so concurrent mutations queue a new flush.
    const pending = Array.from(ids)
    dirtyMetaIds.value = new Set()
    const byId = new Map(conversations.value.map(c => [c.id, c] as const))
    const metas: ConversationMeta[] = []
    for (const id of pending) {
      const c = byId.get(id)
      if (c) metas.push(toConversationMeta(c))
    }
    if (metas.length === 0) return
    await saveConversationMeta(metas).catch(e => console.error('save meta error', e))
  }

  /** P0: append client-held messages missing from DB (never deletes tool rows). */
  function persistAppend(conversationId: string) {
    const conv = conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    const [stripped] = stripEphemeralDesktopNoticesForDisk([conv])
    const messages = JSON.parse(JSON.stringify(stripped.messages)) as ChatMessage[]
    appendConversationMessages(conversationId, messages).catch(e =>
      console.error('append messages error', e)
    )
  }

  function newConversation(): Conversation {
    const existingBlank = conversations.value.find(isBlankDesktopConversation)
    if (existingBlank) {
      existingBlank.updatedAt = Date.now()
      conversations.value = [
        existingBlank,
        ...conversations.value.filter(c => c.id !== existingBlank.id)
      ]
      flushActiveComposerDraft()
      currentId.value = existingBlank.id
      loadActiveComposerDraft(existingBlank.id)
      hydratedIds.value.add(existingBlank.id)
      markMetaDirty(existingBlank.id)
      return existingBlank
    }
    // New sessions start with an empty workspace; backend inherits from the last active
    // conversation on first send unless the user clears the picker (inherit disabled).
    const c: Conversation = {
      id: uid(),
      title: DEFAULT_CONVERSATION_TITLE,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: '',
      workspaceUserSet: false,
      workspaceInheritDisabled: false,
      leadAgentId: DEFAULT_LEAD_AGENT_ID,
      agentMode: 'single'
    }
    conversations.value.unshift(c)
    flushActiveComposerDraft()
    currentId.value = c.id
    loadActiveComposerDraft(c.id)
    hydratedIds.value.add(c.id)
    markMetaDirty(c.id)
    return c
  }

  /**
   * Open a cron job's active isolated session in the main panel. Cron sessions
   * are excluded from the sidebar list (see AppShell's filteredConversations)
   * and from the backend meta pagination, so they are only reachable through
   * this entry. The active session id is `cron:{jobId}:{yyyymmdd}` (advanced on
   * daily rollover); prior ids' transcripts remain on disk but are not exposed
   * here. If the session shell is not yet in the store, a transient shell is
   * built from the job's label/agent fields and injected so the main panel can
   * render it; its real messages are hydrated on demand by selectConversation
   * → ensureMessagesLoaded.
   */
  function openCronConversation(
    sessionId: string,
    label: string,
    leadAgentId?: string | null,
    agentMode?: string | null
  ): void {
    if (!sessionId) {
      console.warn('[chat] openCronConversation: empty sessionId')
      return
    }
    let conv = conversations.value.find(c => c.id === sessionId)
    if (!conv) {
      conv = {
        id: sessionId,
        title: label?.trim() ? `[定时] ${label}` : '[定时] cron 会话',
        createdAt: Date.now(),
        updatedAt: Date.now(),
        messages: [],
        skillIds: [],
        toolRoundsUsed: 0,
        toolRoundsUsedSupervisor: 0,
        workspaceRoot: '',
        workspaceUserSet: false,
        workspaceInheritDisabled: false,
        leadAgentId: leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID,
        agentMode: (agentMode?.trim() || 'single') as AgentMode
      }
      conversations.value.unshift(conv)
    } else if (label?.trim()) {
      // Refresh the title/agent on re-open so a stale shell picks up the
      // task's current label (e.g. after rename) instead of keeping an old one.
      conv.title = `[定时] ${label}`
      conv.leadAgentId = leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
      conv.agentMode = (agentMode?.trim() || 'single') as AgentMode
    }
    // Always re-hydrate from DB: cron shells are transient, hydratedIds may
    // cache an empty snapshot from before the first tick finished, and
    // selectConversation skips work when currentId is already this session.
    hydratedIds.value.delete(sessionId)
    flushActiveComposerDraft()
    currentId.value = sessionId
    reconcileRunStateForConversation(sessionId)
    loadActiveComposerDraft(sessionId)
    void ensureMessagesLoaded(sessionId, { force: true })
    void refreshTaskBoard(sessionId)
    void refreshSubAgentTaskBoards(sessionId)
  }

  /**
   * Open a webhook source's dedicated session in the main panel. Webhook
   * sessions are excluded from the sidebar list and only reachable via the
   * automation panel's view-session entry.
   */
  function openWebhookConversation(sessionId: string, src: string): void {
    if (!sessionId) {
      console.warn('[chat] openWebhookConversation: empty sessionId')
      return
    }
    const label = src?.trim() || sessionId.slice('webhook:'.length)
    let conv = conversations.value.find(c => c.id === sessionId)
    if (!conv) {
      conv = {
        id: sessionId,
        title: label ? `[Webhook] ${label}` : '[Webhook] 会话',
        createdAt: Date.now(),
        updatedAt: Date.now(),
        messages: [],
        skillIds: [],
        toolRoundsUsed: 0,
        toolRoundsUsedSupervisor: 0,
        workspaceRoot: '',
        workspaceUserSet: false,
        workspaceInheritDisabled: false,
        leadAgentId: DEFAULT_LEAD_AGENT_ID,
        agentMode: 'single'
      }
      conversations.value.unshift(conv)
    } else if (label) {
      conv.title = `[Webhook] ${label}`
    }
    hydratedIds.value.delete(sessionId)
    flushActiveComposerDraft()
    currentId.value = sessionId
    reconcileRunStateForConversation(sessionId)
    loadActiveComposerDraft(sessionId)
    void ensureMessagesLoaded(sessionId, { force: true })
    void refreshTaskBoard(sessionId)
    void refreshSubAgentTaskBoards(sessionId)
  }

  function selectConversation(id: string) {
    const conv = conversations.value.find(c => c.id === id)
    const needsHydration = conversationNeedsMessageHydration(conv)
    if (currentId.value === id && !needsHydration) return
    flushActiveComposerDraft()
    currentId.value = id
    reconcileRunStateForConversation(id)
    loadActiveComposerDraft(id)
    void ensureMessagesLoaded(
      id,
      needsHydration && (conv?.messageCount ?? 0) > 0 && (conv?.messages.length ?? 0) === 0
        ? { force: true }
        : undefined
    )
    void refreshTaskBoard(id)
    void refreshSubAgentTaskBoards(id)
  }

  async function deleteConversation(id: string) {
    const conv = conversations.value.find(c => c.id === id)
    if (conv) {
      for (const m of conv.messages) {
        desktopNotice.clearSchedule(m.id)
      }
    }
    const i = conversations.value.findIndex(c => c.id === id)
    if (i >= 0) conversations.value.splice(i, 1)
    hydratedIds.value.delete(id)
    clearOutboundQueue(id)
    cancelGeneratingClearTimer(id)
    const nextRuns = { ...runByConversation.value }
    delete nextRuns[id]
    runByConversation.value = nextRuns
    clearOutboundQueue(id)
    clearRunState(id)
    // Explicitly delete the row + its messages + sandbox. persistMeta() is a
    // pure upsert now (paginated subset), so it can no longer delete for us.
    await deleteConversationApi(id).catch(err =>
      console.error('[chat] deleteConversation api failed', id, err)
    )
    if (currentId.value === id) {
      clearComposerDraft(id)
      currentId.value = conversations.value[0]?.id || null
      if (!currentId.value) newConversation()
      else {
        loadActiveComposerDraft(currentId.value)
        void ensureMessagesLoaded(currentId.value)
      }
    } else {
      clearComposerDraft(id)
    }
    // The deleted row is removed via deleteConversationApi above; remaining
    // conversations are unchanged. Only flush any other pending dirty metas.
    void flushPersistMeta()
  }

  function findMessage(
    messageId: string,
    preferConversationId?: string
  ): { conv: Conversation; msg: ChatMessage } | null {
    const prefer = preferConversationId?.trim() || currentId.value?.trim()
    if (prefer) {
      const conv = conversations.value.find(c => c.id === prefer)
      if (conv) {
        const msg = conv.messages.find(m => m.id === messageId)
        if (msg) return { conv, msg }
      }
    }
    for (const conv of conversations.value) {
      if (!isConversationGenerating(conv.id)) continue
      const msg = conv.messages.find(m => m.id === messageId)
      if (msg) return { conv, msg }
    }
    for (const conv of conversations.value) {
      const msg = conv.messages.find(m => m.id === messageId)
      if (msg) return { conv, msg }
    }
    return null
  }

  function resolveToolCall(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ): ToolCall | null {
    const r = findMessage(messageId)
    if (!r) return null
    const scopedTarget = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
    if (scopedTarget) {
      return scopedTarget.toolCalls?.find(t => t.id === toolCallId) ?? null
    }
    if (traceId?.trim()) {
      const scoped = scopedMessagesForTrace(r.conv.messages, r.msg.id, traceId.trim())
      for (let i = scoped.length - 1; i >= 0; i--) {
        const tc = scoped[i].toolCalls?.find(t => t.id === toolCallId)
        if (tc) return tc
      }
      const trace = ensureSubTrace(r.msg, traceId.trim())
      return trace.session?.toolCalls?.find(t => t.id === toolCallId) ?? null
    }
    return r.msg.toolCalls?.find(t => t.id === toolCallId) ?? null
  }

  const terminalLive = createTerminalLiveManager({
    popup: terminalLivePopup,
    resolveToolCall
  })

  function applyReasoningDeltaBatch(
    messageId: string,
    traceId: string | undefined,
    scopedMessageId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
    if (target) {
      target.reasoning = (target.reasoning || '') + text
      target.contentStreaming = true
      target.status = 'streaming'
      return
    }
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const session = trace.session
      if (session) {
        session.reasoning = (session.reasoning || '') + text
        session.contentStreaming = true
      }
      return
    }
    r.msg.reasoning = (r.msg.reasoning || '') + text
    r.msg.status = 'streaming'
    r.msg.contentStreaming = true
  }

  setReasoningDeltaApplyHandler(applyReasoningDeltaBatch)

  function scheduleDesktopNoticeRemoval(conversationId: string, messageId: string) {
    desktopNotice.scheduleRemoval(conversationId, messageId)
  }

  function handleEvent(e: StreamEvent) {
    try {
      handleEventInner(e)
    } catch (err) {
      console.error('[chat stream] handleEvent failed', err, e)
    }
  }

  const {
    applyTaskBoardDocument,
    applyTaskBoardDocumentDebounced,
    refreshTaskBoard,
    activeParentBoardDocument,
    activeParentBoardBinding,
    compactTaskBoardDocument,
    parentBoardsBoundToMessage,
    childBoardBindingForTrace,
    taskBoardForConversation,
    childBoardsForParent
  } = taskBoardMgr

  function emptyComposerDraft(): ComposerDraft {
    return { text: '', attachments: [] }
  }

  function getComposerDraft(conversationId: string | null): ComposerDraft {
    if (!conversationId) return emptyComposerDraft()
    return composerDraftByConvId.value[conversationId] ?? emptyComposerDraft()
  }

  function setComposerDraft(conversationId: string | null, draft: ComposerDraft) {
    if (!conversationId) return
    const next = { ...composerDraftByConvId.value }
    const isEmpty = !draft.text && draft.attachments.length === 0
    if (isEmpty) delete next[conversationId]
    else {
      next[conversationId] = {
        text: draft.text,
        attachments: draft.attachments.map(a => ({ ...a }))
      }
    }
    composerDraftByConvId.value = next
  }

  function clearComposerDraft(conversationId: string | null) {
    if (!conversationId) return
    const draft = composerDraftByConvId.value[conversationId]
    if (draft) {
      for (const att of draft.attachments) {
        releaseComposerAttachment(att.id)
      }
    }
    const next = { ...composerDraftByConvId.value }
    delete next[conversationId]
    composerDraftByConvId.value = next
  }

  function clearActiveComposer() {
    composerDraftHydrating.value = true
    composerText.value = ''
    composerAttachments.value = []
    if (currentId.value) {
      const next = { ...composerDraftByConvId.value }
      delete next[currentId.value]
      composerDraftByConvId.value = next
    }
    nextTick(() => {
      composerDraftHydrating.value = false
    })
  }

  function flushActiveComposerDraft() {
    if (composerDraftTimer != null) {
      window.clearTimeout(composerDraftTimer)
      composerDraftTimer = null
    }
    const id = currentId.value
    if (!id) return
    setComposerDraft(id, {
      text: composerText.value,
      attachments: composerAttachments.value.map(a => ({ ...a }))
    })
  }

  function scheduleComposerDraftFlush() {
    if (composerDraftHydrating.value) return
    if (composerDraftTimer != null) window.clearTimeout(composerDraftTimer)
    composerDraftTimer = window.setTimeout(() => {
      composerDraftTimer = null
      flushActiveComposerDraft()
    }, 200)
  }

  function loadActiveComposerDraft(conversationId: string | null) {
    composerDraftHydrating.value = true
    const draft = getComposerDraft(conversationId)
    composerText.value = draft.text
    composerAttachments.value = hydrateComposerAttachments(draft.attachments)
    nextTick(() => {
      composerDraftHydrating.value = false
    })
  }

  watch(composerText, () => {
    scheduleComposerDraftFlush()
  })

  watch(composerAttachments, () => {
    scheduleComposerDraftFlush()
  }, { deep: true })

  function prefillComposer(text: string) {
    composerPrefill.value = text
    composerText.value = text
  }

  function consumeComposerPrefill(): string | null {
    const v = composerPrefill.value
    composerPrefill.value = null
    return v
  }

  function showUiToast(message: string, level: 'success' | 'warning' | 'error') {
    uiToast.value = { message, level }
    if (uiToastTimer != null) window.clearTimeout(uiToastTimer)
    uiToastTimer = window.setTimeout(() => {
      uiToast.value = null
      uiToastTimer = null
    }, 4500)
  }

  function streamHandlerContext(): StreamHandlerContext {
    return {
      conversations,
      currentId,
      computerMonitorPickRequest,
      terminalInputRequest,
      ensureImConversation,
      ensureCronStreamConversation,
      findMessage,
      persistMeta,
      markMetaDirty,
      persistAppend,
      showUiToast,
      patchRunState,
      clearRunState,
      clearAllRunStates,
      isConversationGenerating,
      hasInFlightToolCalls,
      scheduleMaybeFinishGenerating,
      applyTaskBoardDocument,
      applyTaskBoardDocumentDebounced,
      refreshTaskBoard,
      handleTerminalToolCallStatus: terminalLive.handleToolCallStatus,
      syncTerminalLivePopupOutput: terminalLive.syncPopupOutput,
      dismissTerminalLivePopup: () => terminalLive.dismiss(),
      clearTerminalInputRequest,
      scheduleDesktopNoticeRemoval,
      applySessionAgentToConversation,
      loadActiveComposerDraft,
      refreshConversationMessages: (conversationId: string) => {
        void ensureMessagesLoaded(conversationId, { force: true })
      }
    }
  }

  function handleEventInner(e: StreamEvent) {
    dispatchStreamEvent(streamHandlerContext(), e)
  }

  async function sendUserMessage(content: string, attachments: ComposerAttachment[] = []) {
    if (!current.value) newConversation()
    const conv = current.value!
    const hasAttachments = attachments.length > 0
    if ((!content.trim() && !hasAttachments)) return
    const platformAuth = usePlatformAuthStore()
    let refreshErrorMessage: string | null = null
    try {
      await platformAuth.ensureFreshSession()
    } catch (e) {
      console.error('[chat] platform session refresh failed', e)
      refreshErrorMessage = e instanceof Error ? e.message : String(e)
    }
    if (!platformAuth.session.logged_in) {
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage:
          refreshErrorMessage ||
          platformAuth.error ||
          (platformAuth.isStandalone ? '请先登录' : '请先登录 Pointer 账户')
      })
      return
    }
    if (platformAuth.tokenQuotaExhausted) {
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage:
          '套餐 Token 额度已用尽，请前往 Openpointer 官网充值或联系管理员。'
      })
      return
    }
    const wireAttachments = []
    for (const a of attachments) {
      const isOssVideo = a.kind === 'video' && !!a.remoteUrl?.trim()
      const contentBase64 = isOssVideo ? undefined : getComposerAttachmentContentBase64(a) ?? undefined
      const previewUrl = getComposerAttachmentDataUrl(a) ?? undefined
      let storageRelPath = a.storageRelPath
      if (contentBase64 && !storageRelPath) {
        try {
          storageRelPath = await saveChatAttachment({
            conversationId: conv.id,
            attachmentId: a.id,
            contentBase64,
            fileName: a.fileName
          })
        } catch (e) {
          console.warn('[chat] saveChatAttachment failed', e)
        }
      }
      const { previewUrl: _p, contentBase64: _c, uploadState: _u, uploadProgress: _up, uploadError: _ue, localSourcePath: _lp, ...rest } = a
      wireAttachments.push({
        ...rest,
        ...(contentBase64 ? { contentBase64 } : {}),
        ...(previewUrl ? { previewUrl } : {}),
        ...(storageRelPath ? { storageRelPath } : {}),
        ...(a.remoteUrl ? { remoteUrl: a.remoteUrl } : {}),
        ...(a.ossObjectKey ? { ossObjectKey: a.ossObjectKey } : {})
      })
    }
    const userMsg: ChatMessage = {
      id: uid(), role: 'user', content,
      status: 'done', createdAt: Date.now(),
      ...(wireAttachments.length ? { attachments: wireAttachments } : {})
    }

    if (isConversationGenerating(conv.id)) {
      enqueueOutbound(conv.id, {
        id: userMsg.id,
        content,
        createdAt: userMsg.createdAt,
        ...(wireAttachments.length ? { attachments: wireAttachments } : {})
      })
      for (const att of attachments) {
        releaseComposerAttachment(att.id)
      }
      showUiToast(`已加入队列（${outboundQueueCount(conv.id)} 条待发送）`, 'success')
      return
    }

    conv.messages.push(userMsg)
    maybeUpdateConversationTitle(conv)
    for (const att of attachments) {
      releaseComposerAttachment(att.id)
    }
    conv.updatedAt = Date.now()

    await dispatchChatTurn(conv)
  }

  async function stop() {
    if (!current.value) return
    const conv = current.value
    const msgId = runStateFor(conv.id).activeMessageId
    flushReasoningDeltaBuffer(msgId ?? undefined)
    terminalLive.clear()
    await cancelChat(conv.id).catch(e => console.error(e))
    clearRunState(conv.id)
    if (msgId) {
      const row = conv.messages.find(m => m.id === msgId)
      if (row?.role === 'assistant' && (row.status === 'streaming' || row.status === 'pending')) {
        row.status = 'done'
      }
    }
    removeDiscardableAssistant(conv, msgId)
    markMetaDirty(conv.id)
  }

  async function abortTerminalOnly(toolCallId?: string) {
    if (!current.value) return
    await abortTerminalCommand(current.value.id, toolCallId).catch(e => console.error(e))
  }

  async function approve(toolCall: ToolCall, approved: boolean) {
    if (!current.value) return
    await approveToolCall(current.value.id, toolCall.id, approved)
      .catch(e => console.error(e))
  }

  function setConversationAgent(leadAgentId: string, agentMode: AgentMode = 'single') {
    const conv = current.value ?? newConversation()
    applySessionAgentToConversation(conv, leadAgentId, agentMode)
    markMetaDirty(conv.id)
  }

  function clearPlatformLoginErrorMessages() {
    const conv = current.value
    if (!conv) return
    const before = conv.messages.length
    conv.messages = conv.messages.filter(m => !isPlatformLoginErrorMessage(m))
    if (conv.messages.length !== before) {
      conv.updatedAt = Date.now()
    }
  }

  function setConversationWorkspace(root: string) {
    if (!current.value) newConversation()
    if (!current.value) return
    const trimmed = root.trim()
    current.value.workspaceRoot = trimmed
    if (trimmed.length > 0) {
      current.value.workspaceUserSet = true
      current.value.workspaceInheritDisabled = false
    } else {
      current.value.workspaceUserSet = false
      current.value.workspaceInheritDisabled = true
    }
    markMetaDirty(current.value.id)
  }

  function clearComputerMonitorPickRequest() {
    computerMonitorPickRequest.value = null
  }

  function clearTerminalInputRequest(toolCallId?: string) {
    const req = terminalInputRequest.value
    if (!req) return
    if (!toolCallId || req.toolCallId === toolCallId) {
      terminalInputRequest.value = null
    }
  }

  function dismissTerminalInputModal() {
    terminalInputRequest.value = null
  }

  function dismissTerminalLivePopup() {
    terminalLive.dismiss()
  }

  function resetForPlatformLogout() {
    conversations.value = []
    currentId.value = null
    nextCursor.value = null
    hydratedIds.value = new Set()
    messagesLoadingIds.value = new Set()
    dirtyMetaIds.value = new Set()
    if (composerDraftTimer != null) {
      window.clearTimeout(composerDraftTimer)
      composerDraftTimer = null
    }
    composerDraftByConvId.value = {}
    clearActiveComposer()
    clearAllRunStates()
    outboundQueues.value = {}
    taskBoards.value = {}
    newConversation()
  }

  return {
    conversations, currentId, current, currentOutboundQueue, isCurrentConversationHydrating, generating, activeGeneratingMessageId, isConversationGenerating, outboundQueueItems, outboundQueueCount, removeOutboundQueueItem, uiToast, taskBoards,
    init, resetForPlatformLogout, newConversation, openCronConversation, openWebhookConversation, selectConversation, deleteConversation,
    loadMoreConversations, loadingMoreConversations, hasMoreConversations,
    ensureMessagesLoaded,
    sendUserMessage, stop, abortTerminalOnly, approve,
    refreshTaskBoard, refreshSubAgentTaskBoards, taskBoardForConversation, activeParentBoardDocument, activeParentBoardBinding, compactTaskBoardDocument, parentBoardsBoundToMessage,
    childBoardBindingForTrace, childBoardsForParent, lookupChildTaskBoard,
    setConversationWorkspace, setConversationAgent,
    effectiveConversationLeadAgentId, effectiveConversationAgentMode,
    showUiToast,
    clearPlatformLoginErrorMessages,
    composerPrefill, prefillComposer, consumeComposerPrefill,
    composerText, composerAttachments, clearActiveComposer,
    getComposerDraft, setComposerDraft, clearComposerDraft,
    computerMonitorPickRequest, clearComputerMonitorPickRequest,
    terminalInputRequest, dismissTerminalInputModal,
    terminalLivePopup, dismissTerminalLivePopup
  }
})
