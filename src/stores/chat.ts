import { defineStore } from 'pinia'
import { ref, computed, watch, nextTick } from 'vue'
import {
  sendChat, cancelChat, abortTerminalCommand, approveToolCall, onStream,
  waitForChatStreamReady,
  loadConversationMetas,
  loadConversationMeta,
  loadConversationMessagesPage,
  DEFAULT_MESSAGE_PAGE_TURNS,
  saveConversationMeta,
  deleteConversation as deleteConversationApi,
  appendConversationMessages,
  saveChatAttachment,
  getDispatcherQueueSnapshot,
  loadProjects,
  loadProject as loadProjectApi,
  loadProjectConversationMetas,
  deleteProject as deleteProjectApi
} from '../lib/api'
import type {
  AgentMode,
  ChatMessage,
  ComputerMonitorPickRequest,
  Conversation,
  ConversationCursor,
  ConversationMeta,
  ConversationMetaPage,
  Project,
  OutboundQueueItem,
  StreamEvent,
  TerminalInputRequest,
  ToolCall,
  TaskBoardDocument
} from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { CODER_AGENT_ID, GENERAL_AGENT_ID } from '../lib/agentUi'
import { promoteOutboundQueueItem } from '../lib/outboundQueue'
import { getTaskBoardSnapshot } from '../lib/api'
import { withRetries } from '../lib/retry'
import { subTaskIdFromTraceId } from '../lib/subAgentStats'
import { resolveStreamWriteMessage, rehydrateAgentTracesFromScopedMessages, scopedMessagesForTrace } from '../lib/subAgentMessages'
import { stripWireAttachmentFields } from '../lib/messageNormalizer'
import {
  clearLastConversationId,
  readLastConversationId,
  writeLastConversationId
} from '../lib/lastConversation'
import {
  conversationNeedsHydration,
  messagesForChatDispatch,
  messagesForPersistAppend,
  persistedCandidateMessageIds
} from '../lib/chatDispatchHistory'
import { hasActiveTurn, peekActiveTurn, recordTurnDone, recordTurnStart } from '../lib/turnElapsed'
import {
  clearStreamDeltaBuffers,
  flushStreamDeltaBuffers,
  setContentDeltaApplyHandler,
  setReasoningDeltaApplyHandler,
  setToolArgsDeltaApplyHandler,
  setToolOutputDeltaApplyHandler,
  setWebSearchOutputDeltaApplyHandler
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
  isDiscardableEmptyAssistant,
  isEphemeralDesktopNoticeMessage
} from '../lib/assistantMessageKind'
import {
  ensureSubTrace,
  migrateLegacyTraceUiState
} from '../lib/subAgentSession'
import { useSettingsStore } from './settings'
import { usePlatformAuthStore } from './platformAuth'
import { isTauriRuntime } from '../lib/runtime'
import { disarmTaskCompleteAudio, primeTaskCompleteAudio } from '../lib/taskCompleteSound'
import { dispatchStreamEvent, type StreamHandlerContext } from './chat/streamHandlers/dispatch'
import {
  assistantTurnActivelyRunning,
  computeHistoryTrimCutByViewedAt,
  hasInFlightToolCalls,
  normalizeInterruptedAssistantStatuses,
  normalizeStaleEndedAssistantTurn,
  removeAssistantMessage,
  uid
} from './chat/helpers'
import { activeConversationIdsFromQueueSnapshot } from './chat/dispatcherRunSync'

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
        // Sub-agent frames default collapsed (running and terminal); keep user expands.
        if (!trace.userExpanded) {
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

interface ContextCompressingState {
  scope: string
  messageId?: string
  subAgentId?: string
  subAgentName?: string
  startedAt: number
}

interface ConversationRunState {
  generating: boolean
  activeMessageId: string | null
  /** Ephemeral in-thread marker while context compression LLM runs. */
  contextCompressing: ContextCompressingState | null
}

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  /** Persisted project sidebar, independent of the loaded recent-conversation page. */
  const projects = ref<Project[]>([])
  /** First sidebar page is 5 projects; true when another page exists. */
  const SIDEBAR_PROJECT_PAGE_SIZE = 5
  const hasMoreProjects = ref(false)
  const loadingMoreProjects = ref(false)
  /** Project details loaded for active conversations without changing sidebar membership. */
  const projectDetails = ref<Record<string, Project>>({})
  const projectLoads = new Map<string, Promise<Project | null>>()
  const currentId = ref<string | null>(null)
  const runByConversation = ref<Record<string, ConversationRunState>>({})

  /** FIFO outbound sends waiting while the session turn is still running (Hermes-style). */
  const outboundQueues = ref<Record<string, OutboundQueueItem[]>>({})
  /**
   * After interrupt / force-send, the cancelled run may still emit a late `Done`.
   * First Done while a newer turn is already active must not close that turn's timing
   * or clear its generating state.
   */
  const pendingInterruptDoneAt = new Map<string, number>()
  /** Conversations that finished while not focused — sidebar shows a solid dot until opened. */
  const awaitingViewIds = ref<Set<string>>(new Set())

  function isConversationAwaitingView(id: string): boolean {
    const key = id.trim()
    return !!key && awaitingViewIds.value.has(key)
  }

  function markConversationAwaitingView(conversationId: string) {
    const key = conversationId.trim()
    if (!key) return
    if (key === (currentId.value?.trim() || '')) return
    if (awaitingViewIds.value.has(key)) return
    const next = new Set(awaitingViewIds.value)
    next.add(key)
    awaitingViewIds.value = next
    console.info('[chat] mark conversation awaiting view', key)
  }

  function clearConversationAwaitingView(conversationId: string) {
    const key = conversationId.trim()
    if (!key || !awaitingViewIds.value.has(key)) return
    const next = new Set(awaitingViewIds.value)
    next.delete(key)
    awaitingViewIds.value = next
  }

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

  /**
   * Pause the active turn and send a queued item immediately (Hermes-style interrupt).
   * Promotes the item to the front, stops the current run (same as composer Stop), then drains.
   */
  async function forceSendOutbound(conversationId: string, itemId: string) {
    const key = conversationId.trim()
    const id = itemId.trim()
    if (!key || !id) return
    const queue = outboundQueues.value[key]
    if (!queue?.some(item => item.id === id)) return

    const promoted = promoteOutboundQueueItem(queue, id)
    outboundQueues.value = { ...outboundQueues.value, [key]: promoted }

    const conv = conversations.value.find(c => c.id === key)
    const hasActiveTurn =
      isConversationGenerating(key) ||
      (!!conv &&
        [...conv.messages]
          .reverse()
          .some(m => m.role === 'assistant' && assistantTurnActivelyRunning(m)))

    if (hasActiveTurn) {
      showUiToast('已暂停当前任务，正在立即发送…', 'warning')
      await interruptActiveTurn(key)
      return
    }

    await drainOutboundQueue(key)
  }

  /**
   * End the active turn — same host cancel as the composer Stop button.
   * Awaits `cancelChat` before draining so a force-send cannot start a new turn
   * while the previous run is still alive.
   */
  async function interruptActiveTurn(conversationId: string) {
    const key = conversationId.trim()
    if (!key) return
    const conv = conversations.value.find(c => c.id === key)
    const msgId = runStateFor(key).activeMessageId
    flushStreamDeltaBuffers(msgId ?? undefined)
    if (current.value?.id === key) {
      terminalLive.clear()
    }

    // Close work-time for the interrupted turn before the next dispatch overwrites it.
    if (hasActiveTurn(key)) {
      recordTurnDone(key)
    }
    const interruptAt = Date.now()
    pendingInterruptDoneAt.set(key, interruptAt)

    // Optimistic UI: hide stop button / show cancelled while host cancel runs.
    patchRunState(key, {
      generating: false,
      activeMessageId: null,
      contextCompressing: null
    })

    if (conv && msgId) {
      const row = conv.messages.find(m => m.id === msgId)
      if (row?.role === 'assistant') {
        // Always keep a cancelled row so「已停止生成」stays visible (compact inline).
        row.status = 'cancelled'
        row.errorMessage = '已停止生成'
        row.contentStreaming = false
        for (const tc of row.toolCalls ?? []) {
          if (tc.status === 'pending_approval') {
            tc.status = 'rejected'
          } else if (tc.status === 'running' || tc.status === 'pending') {
            tc.status = tc.result?.trim() ? 'success' : 'failed'
            if (tc.status === 'failed' && !tc.error) tc.error = 'interrupted'
          }
        }
        markMetaDirty(key)
      }
    }

    try {
      await cancelChat(key)
    } catch (e) {
      console.error('[chat] cancelChat failed', e)
    }

    // Host cancel is signaled; keep UI stopped even if a late stream event raced.
    patchRunState(key, {
      generating: false,
      activeMessageId: null,
      contextCompressing: null
    })
    disarmTaskCompleteAudio()
    await drainOutboundQueue(key)

    // If the cancelled run never emitted Done, drop the watch so the next turn's Done is kept.
    window.setTimeout(() => {
      if (pendingInterruptDoneAt.get(key) === interruptAt) {
        pendingInterruptDoneAt.delete(key)
        console.info('[chat] interrupt Done watch expired', key)
      }
    }, 3000)
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
  /** Tool call id whose live-output「查看」is eligible (after 5s + has output). */
  const terminalLiveViewReadyToolCallId = ref<string | null>(null)
  let uiToastTimer: number | null = null
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
  /**
   * Turn-page window metadata per conversation (tail / around / prepend).
   * `hydratedIds` means the current window is ready — not that the full transcript is in RAM.
   */
  type MessagePageState = {
    hasMoreOlder: boolean
    hasMoreNewer: boolean
    oldestPosition: number | null
    newestPosition: number | null
    loadingOlder: boolean
  }
  const messagePageByConv = ref<Record<string, MessagePageState>>({})
  const olderLoadPromises = new Map<string, Promise<boolean>>()
  /** Last-viewed timestamp per user message (current-conversation history trim). */
  const EMPTY_VIEWED_MAP: ReadonlyMap<string, number> = new Map()
  /** How long a user message can go unviewed before it becomes trim candidate. */
  const TRIM_HISTORY_STALE_MS = 60 * 60 * 1000
  /** Always keep at least this many user turns in memory (3 pages × 8 turns). */
  const TRIM_HISTORY_MIN_KEEP_TURNS = 24
  const messageViewedAtByConv = new Map<string, Map<string, number>>()
  /**
   * Message ids already known to exist in SQLite for a hydrated conversation.
   * Used so `sendChat` only ships rows that still need `append_missing`.
   */
  const persistedMessageIdsByConv = new Map<string, Set<string>>()
  /** Conversation ids currently fetching messages from disk. */
  const messagesLoadingIds = ref<Set<string>>(new Set())
  /** Shared hydration promises let send paths wait for an in-flight project/shell load. */
  const messageHydrationPromises = new Map<string, Promise<boolean>>()

  function persistedIdsFor(convId: string): Set<string> {
    let set = persistedMessageIdsByConv.get(convId)
    if (!set) {
      set = new Set()
      persistedMessageIdsByConv.set(convId, set)
    }
    return set
  }

  function replacePersistedMessageIds(convId: string, ids: Iterable<string>) {
    persistedMessageIdsByConv.set(convId, new Set(ids))
  }

  function addPersistedMessageIds(convId: string, ids: Iterable<string>) {
    const set = persistedIdsFor(convId)
    for (const id of ids) {
      if (id) set.add(id)
    }
  }

  function clearPersistedMessageIds(convId: string) {
    persistedMessageIdsByConv.delete(convId)
  }

  function markConversationMessagesPersisted(convId: string) {
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) return
    replacePersistedMessageIds(convId, persistedCandidateMessageIds(conv.messages))
  }
  /** Sidebar search (or similar) asks MessageList to scroll to this message after open/hydrate. */
  const pendingFocusMessage = ref<{
    conversationId: string
    messageId: string
    queryTerm?: string
  } | null>(null)

  function clearPendingFocusMessage() {
    pendingFocusMessage.value = null
  }

  // ── Idle conversation eviction ──
  /** Minutes of inactivity before a conversation's messages are evicted from memory. Set to 0 to disable. */
  const IDLE_EVICTION_MINUTES = 120
  /** Timestamp (Date.now()) of the last time each conversation was selected. */
  const lastAccessed = new Map<string, number>()

  function touchConversation(id: string) {
    lastAccessed.set(id, Date.now())
  }

  function evictConversation(id: string) {
    const conv = conversations.value.find(c => c.id === id)
    if (!conv) return
    if (isConversationGenerating(id)) return
    if (currentId.value === id) return
    if (conv.messages.length === 0) return
    console.info('[chat] evicting idle conversation', id, conv.title, 'messages', conv.messages.length)
    conv.messages = []
    hydratedIds.value.delete(id)
    clearPersistedMessageIds(id)
    const nextPages = { ...messagePageByConv.value }
    delete nextPages[id]
    messagePageByConv.value = nextPages
    lastAccessed.delete(id)
  }

  /** Evict all conversations idle longer than `IDLE_EVICTION_MINUTES`. Called on each conversation switch. */
  function evictIdleConversations() {
    if (IDLE_EVICTION_MINUTES <= 0) return
    const cutoff = Date.now() - IDLE_EVICTION_MINUTES * 60_000
    for (const conv of conversations.value) {
      const last = lastAccessed.get(conv.id)
      if (last !== undefined && last < cutoff) {
        evictConversation(conv.id)
      }
    }
  }

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
    return conversationNeedsHydration({
      messageCount: conv.messageCount ?? 0,
      messagesLength: conv.messages.length,
      hydrated: hydratedIds.value.has(conv.id),
      loading: messagesLoadingIds.value.has(conv.id)
    })
  }

  const isCurrentConversationHydrating = computed(() =>
    conversationNeedsMessageHydration(current.value)
  )

  /** True when there are more conversation metas to fetch from the DB. */
  const hasMoreConversations = computed(() => nextCursor.value !== null)

  function runStateFor(id: string): ConversationRunState {
    return (
      runByConversation.value[id] ?? {
        generating: false,
        activeMessageId: null,
        contextCompressing: null
      }
    )
  }

  function clearRunState(id: string) {
    const key = id.trim()
    if (!key) return
    patchRunState(key, {
      generating: false,
      activeMessageId: null,
      contextCompressing: null
    })
    queueMicrotask(() => {
      void drainOutboundQueue(key)
    })
  }

  function patchRunState(id: string, patch: Partial<ConversationRunState>) {
    const key = id.trim()
    runByConversation.value = {
      ...runByConversation.value,
      [key || id]: { ...runStateFor(id), ...patch }
    }
  }

  let drainingOutbound = new Set<string>()

  async function drainOutboundQueue(conversationId: string) {
    const convId = conversationId.trim()
    if (!convId || isConversationGenerating(convId) || drainingOutbound.has(convId)) return

    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      console.warn('[chat] outbound drain paused: missing conversation', convId)
      return
    }
    if (conversationNeedsMessageHydration(conv)) {
      const hydrated = await ensureMessagesLoaded(convId, { force: true })
      if (!hydrated) {
        console.error('[chat] outbound drain paused: hydration failed', convId)
        showUiToast('历史消息加载失败，待发送消息已保留', 'error')
        return
      }
    }

    const item = dequeueOutbound(convId)
    if (!item) {
      return
    }

    drainingOutbound.add(convId)
    try {
      // Use dispatch time, not enqueue time — otherwise turn elapsed / time chips
      // include queue wait (especially visible after「立即发送」).
      const userMsg: ChatMessage = {
        id: item.id,
        role: 'user',
        content: item.content,
        status: 'done',
        createdAt: Date.now(),
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
    // Queue drain / force-send may not go through composer click; re-arm audio here.
    primeTaskCompleteAudio()
    const turnId = [...conv.messages].reverse().find(message => message.role === 'user')?.id
    if (turnId) recordTurnStart(conv.id, turnId)
    patchRunState(conv.id, {
      generating: true,
      activeMessageId: null,
      contextCompressing: null
    })
    clearConversationAwaitingView(conv.id)
    await flushPersistMeta()
    await refreshTaskBoard(conv.id)

    const hydrated = hydratedIds.value.has(conv.id)
    const persistedIds = hydrated ? persistedIdsFor(conv.id) : undefined
    let history = messagesForChatDispatch(
      conv.messages,
      persistedIds ? { persistedIds } : undefined
    )
    if (hydrated && history.length === 0) {
      console.warn(
        '[chat] incremental dispatch empty; falling back to full history',
        conv.id,
        'messages',
        conv.messages.length
      )
      history = messagesForChatDispatch(conv.messages)
    } else if (hydrated) {
      console.info(
        '[chat] dispatch incremental history',
        conv.id,
        history.length,
        'of',
        conv.messages.length
      )
    }

    try {
      // Web: avoid POST before SSE has receivers (new chat first turn race).
      await waitForChatStreamReady()
      await withRetries(
        async () =>
          await sendChat({
            conversationId: conv.id,
            messages: history,
            // Skills resolve on the backend from user_settings.agentSkillOverrides.
            enabledSkillIds: [],
            agentMode: effectiveConversationAgentMode(conv),
            leadAgentId: effectiveConversationLeadAgentId(conv),
            toolRoundsUsed: 0,
            toolRoundsUsedSupervisor: 0,
            workspaceRoot: conv.workspaceInheritDisabled ? '' : (conv.workspaceRoot ?? '').trim(),
            ...(conv.workspaceInheritDisabled ? { workspaceInheritDisabled: true } : {})
          }),
        {
          onRetry: (err, nextAttempt, delayMs) => {
            console.warn('[chat] sendChat retry', { nextAttempt, delayMs, err })
          }
        }
      )
      // begin() appends these ids before the stream runs; keep the watermark in sync.
      addPersistedMessageIds(conv.id, history.map(m => m.id))
    } catch (err) {
      clearRunState(conv.id)
      console.error('sendChat error', err)
      const errText = String(err)
      if (
        errText.includes('token_quota_exhausted') ||
        errText.includes('账户余额已用尽') ||
        errText.includes('账户余额不足')
      ) {
        usePlatformAuthStore().markTokenQuotaExhausted()
      }
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage: errText.includes('token_quota_exhausted') || errText.includes('账户余额已用尽')
          ? '账户余额已用尽'
          : errText
      })
      persistAppend(conv.id)
    }
  }

  function clearAllRunStates() {
    runByConversation.value = {}
  }

  function isConversationGenerating(id: string): boolean {
    return runStateFor(id).generating
  }

  /** Restore generating UI from server dispatcher queue after page refresh / SSE gap. */
  async function syncRunStateFromDispatcherQueue(mode: 'flags' | 'catch_up' = 'flags') {
    try {
      const snapshot = await getDispatcherQueueSnapshot()
      const activeIds = activeConversationIdsFromQueueSnapshot(snapshot)
      const clearedStaleIds: string[] = []

      // Clear UI that still shows "running" after Done was dropped on a weak link.
      for (const [convId, state] of Object.entries(runByConversation.value)) {
        if (!state.generating) continue
        if (activeIds.has(convId)) continue
        clearRunState(convId)
        const conv = conversations.value.find(c => c.id === convId)
        if (conv) normalizeInterruptedAssistantStatuses([conv])
        clearedStaleIds.push(convId)
        console.info('[chat] syncRunStateFromDispatcherQueue: clear stale', convId)
      }

      for (const convId of activeIds) {
        if (isConversationGenerating(convId)) continue
        patchRunState(convId, {
          generating: true,
          activeMessageId: runStateFor(convId).activeMessageId,
        })
        console.info('[chat] syncRunStateFromDispatcherQueue: active', convId)
      }

      // Run finished on server but UI never saw stream frames (e.g. first message
      // before SSE connected) — pull DB even on flags-only reconcile.
      for (const convId of clearedStaleIds) {
        if (!conversations.value.some(c => c.id === convId)) continue
        void ensureMessagesLoaded(convId, { force: true, silent: true })
      }

      // Pull messages only after a real SSE disconnect / lag — never on poll / visibility.
      if (mode !== 'catch_up') return

      const reloadIds = new Set<string>([...activeIds, ...clearedStaleIds])
      if (currentId.value) reloadIds.add(currentId.value)
      for (const convId of reloadIds) {
        if (!conversations.value.some(c => c.id === convId)) continue
        // clearedStaleIds already force-hydrated above; still fine to call again
        // (deduped by messageHydrationPromises / hydrated short-circuit with force).
        void ensureMessagesLoaded(convId, { force: true, silent: true })
      }
      console.info(
        '[chat] syncRunStateFromDispatcherQueue: catch_up reload',
        [...reloadIds]
      )
    } catch (err) {
      console.warn('[chat] syncRunStateFromDispatcherQueue failed', err)
    }
  }

  let streamGapResyncTimer: number | null = null
  let streamGapWired = false

  /** True only for SSE transport failures — the only paths that force-pull messages. */
  function streamGapNeedsMessageCatchUp(reason: string): boolean {
    return (
      reason === 'server_lagged'
      || reason === 'stream_ended'
      || reason === 'stream_ended_idle'
      || reason === 'stream_error'
      || reason === 'stream_gateway_error'
      || reason === 'sse_gap'
    )
  }

  /**
   * Reconcile run flags with the dispatcher.
   * Message catch-up (`force` hydrate) only when SSE actually dropped / lagged.
   */
  function resyncAfterStreamGap(reason: string) {
    const mode = streamGapNeedsMessageCatchUp(reason) ? 'catch_up' : 'flags'
    console.info('[chat] resyncAfterStreamGap', reason, mode)
    if (streamGapResyncTimer != null) {
      window.clearTimeout(streamGapResyncTimer)
    }
    streamGapResyncTimer = window.setTimeout(() => {
      streamGapResyncTimer = null
      void syncRunStateFromDispatcherQueue(mode)
    }, 300)
  }

  function wireStreamGapRecovery() {
    if (streamGapWired || typeof window === 'undefined') return
    streamGapWired = true
    // Online / visibility: fix stuck stop button only — do not re-fetch messages.
    window.addEventListener('online', () => resyncAfterStreamGap('online'))
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') {
        resyncAfterStreamGap('visibility')
      }
    })
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

  /** Current conversation's ephemeral context-compression marker (auto-clears when done). */
  const contextCompressing = computed(() => {
    const id = currentId.value
    return id ? runStateFor(id).contextCompressing : null
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

  function compareConversationsByPinThenActivity(
    a: { isPinned?: boolean; updatedAt: number; id: string },
    b: { isPinned?: boolean; updatedAt: number; id: string }
  ): number {
    return Number(!!b.isPinned) - Number(!!a.isPinned)
      || b.updatedAt - a.updatedAt
      || b.id.localeCompare(a.id)
  }

  function sortConversationsInPlace(): void {
    conversations.value = [...conversations.value].sort(compareConversationsByPinThenActivity)
  }

  function metaToConversationShell(m: ConversationMeta): Conversation {
    return {
      id: m.id,
      title: m.title,
      createdAt: m.createdAt,
      updatedAt: m.updatedAt,
      isPinned: !!m.isPinned,
      messages: [],
      skillIds: m.skillIds ?? [],
      toolRoundsUsed: m.toolRoundsUsed,
      toolRoundsUsedSupervisor: m.toolRoundsUsedSupervisor,
      computerMonitorId: m.computerMonitorId,
      projectId: m.projectId,
      workspaceRoot: m.workspaceRoot,
      workspaceUserSet: m.workspaceUserSet,
      workspaceInheritDisabled: m.workspaceInheritDisabled,
      leadAgentId: m.leadAgentId,
      agentMode: m.agentMode,
      messageCount: m.messageCount
    }
  }

  /**
   * Prefer the last selected conversation on boot. If it is outside the first
   * meta page, fetch its meta and inject a shell; otherwise fall back to the
   * newest sidebar row.
   */
  async function resolveBootConversationId(shells: Conversation[]): Promise<string> {
    const fallback = shells[0]!.id
    const lastId = readLastConversationId()
    if (!lastId) return fallback
    if (conversations.value.some(c => c.id === lastId)) {
      console.info('[chat] boot: restore last conversation from loaded metas', lastId)
      return lastId
    }
    try {
      const meta = await loadConversationMeta(lastId)
      if (!meta) {
        console.info('[chat] boot: last conversation missing; using newest', lastId)
        clearLastConversationId(lastId)
        return fallback
      }
      const shell = metaToConversationShell(meta)
      if (!shell.leadAgentId?.trim()) shell.leadAgentId = DEFAULT_LEAD_AGENT_ID
      if (!shell.agentMode?.trim()) shell.agentMode = 'single'
      conversations.value = [shell, ...conversations.value]
      sortConversationsInPlace()
      console.info('[chat] boot: restored last conversation outside first page', lastId)
      return lastId
    } catch (err) {
      console.warn('[chat] boot: loadConversationMeta failed; using newest', lastId, err)
      return fallback
    }
  }

  async function init() {
    // Kick off global SSE immediately so the handshake overlaps project/meta
    // hydration — otherwise first send waits on a late connect.
    const streamSetup = !unlisten
      ? onStream(handleEvent, 'global', reason => resyncAfterStreamGap(reason))
          .then(fn => {
            unlisten = fn
          })
          .catch(err => {
            console.error('[chat] onStream failed at boot', err)
          })
      : Promise.resolve()
    wireStreamGapRecovery()

    await refreshProjects()
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
    sortConversationsInPlace()
    nextCursor.value = firstPage.nextCursor
    // 置顶 section renders every pinned meta (5-row scroll viewport only).
    // Drain further pages while the trailing row is still pinned.
    await ensureAllPinnedMetasLoaded()
    // IM conversations can derive a title purely from their id (no messages
    // needed); persist any such title fixes as targeted delta writes. Desktop
    // blank pruning needs no upsert here — pruned blanks are deleted explicitly
    // below via deleteConversationApi, and retained conversations are unchanged.
    for (const conv of conversations.value) {
      if (maybeUpdateConversationTitle(conv)) markMetaDirty(conv.id)
    }
    // Explicitly delete any duplicate blank conversations pruned above (only
    // those we actually saw this boot).
    for (const blankId of pruned.prunedBlankIds) {
      clearLastConversationId(blankId)
      void deleteConversationApi(blankId).catch(err =>
        console.error('[chat] boot prune: deleteConversation failed', blankId, err)
      )
    }

    if (shells.length === 0) {
      newConversation()
    } else {
      const bootId = await resolveBootConversationId(shells)
      const bootConv =
        conversations.value.find(c => c.id === bootId) ?? shells[0]!
      currentId.value = bootConv.id
      writeLastConversationId(bootConv.id)
      touchConversation(bootConv.id)
      await ensureConversationProjectLoaded(bootConv)
      await syncRunStateFromDispatcherQueue()
      await ensureMessagesLoaded(bootConv.id)
      loadActiveComposerDraft(currentId.value)
    }

    await streamSetup
    if (currentId.value) {
      void refreshTaskBoard(currentId.value)
      void refreshSubAgentTaskBoards(currentId.value)
    }
  }

  function projectById(projectId?: string): Project | undefined {
    const id = projectId?.trim()
    if (!id) return undefined
    return projects.value.find(project => project.id === id) ?? projectDetails.value[id]
  }

  function compareProjectsByActivity(a: Project, b: Project): number {
    return Number(b.isPinned) - Number(a.isPinned)
      || b.lastActivityAt - a.lastActivityAt
      || b.id.localeCompare(a.id)
  }

  function syncProjectActivityInMemory(conv: Conversation): void {
    const projectId = conv.projectId?.trim()
    if (!projectId) return
    const sidebarProject = projects.value.find(project => project.id === projectId)
    if (sidebarProject && conv.updatedAt > sidebarProject.lastActivityAt) {
      sidebarProject.lastActivityAt = conv.updatedAt
      projects.value = [...projects.value].sort(compareProjectsByActivity)
    }
    const detail = projectDetails.value[projectId]
    if (detail && conv.updatedAt > detail.lastActivityAt) {
      projectDetails.value = {
        ...projectDetails.value,
        [projectId]: { ...detail, lastActivityAt: conv.updatedAt }
      }
    }
  }

  async function ensureProjectLoaded(projectId: string): Promise<Project | null> {
    const id = projectId.trim()
    if (!id) return null
    const existing = projectById(id)
    if (existing) return existing
    const inFlight = projectLoads.get(id)
    if (inFlight) return inFlight

    const load = loadProjectApi(id)
      .then(project => {
        if (!project) {
          console.warn('[chat] conversation project is unavailable', { projectId: id })
          return null
        }
        projectDetails.value = { ...projectDetails.value, [id]: project }
        return project
      })
      .catch(err => {
        console.warn('[chat] load conversation project failed', { projectId: id, error: err })
        return null
      })
      .finally(() => projectLoads.delete(id))
    projectLoads.set(id, load)
    return load
  }

  async function ensureConversationProjectLoaded(conv: Conversation): Promise<void> {
    if (!conv.projectId) return
    const project = await ensureProjectLoaded(conv.projectId)
    if (project) syncConversationProjectWorkspace(conv)
  }

  async function refreshProjects() {
    try {
      // Same first page as「加载更多项目」so hasMoreProjects is correct without a click.
      const page = await loadProjects(null, SIDEBAR_PROJECT_PAGE_SIZE)
      projects.value = page.items.sort(compareProjectsByActivity)
      hasMoreProjects.value = page.nextCursor !== null
      console.info(
        '[chat] refreshProjects: count=%s hasMore=%s',
        projects.value.length,
        hasMoreProjects.value
      )
    } catch (err) {
      console.error('[chat] loadProjects (sidebar page) failed', err)
      projects.value = []
      hasMoreProjects.value = false
    }
  }

  async function loadMoreProjects() {
    if (loadingMoreProjects.value || !hasMoreProjects.value) return
    const lastProject = projects.value[projects.value.length - 1]
    loadingMoreProjects.value = true
    try {
      const page = await loadProjects(
        lastProject
          ? { lastActivityAt: lastProject.lastActivityAt, id: lastProject.id }
          : null,
        SIDEBAR_PROJECT_PAGE_SIZE
      )
      const knownIds = new Set(projects.value.map(project => project.id))
      const fresh = page.items.filter(project => !knownIds.has(project.id))
      if (fresh.length > 0) {
        projects.value = [...projects.value, ...fresh].sort(compareProjectsByActivity)
      }
      hasMoreProjects.value = page.nextCursor !== null
      console.info(
        '[chat] loadMoreProjects: appended=%s hasMore=%s',
        fresh.length,
        hasMoreProjects.value
      )
    } catch (err) {
      console.error('[chat] loadMoreProjects failed', err)
      throw err
    } finally {
      loadingMoreProjects.value = false
    }
  }

  /** Delete a project and reconcile the active conversation with a valid fallback. */
  async function deleteProject(projectId: string): Promise<void> {
    const id = projectId.trim()
    if (!id) return

    await deleteProjectApi(id)
    const remainingProjectDetails = { ...projectDetails.value }
    delete remainingProjectDetails[id]
    projectDetails.value = remainingProjectDetails
    await refreshProjects()

    const deletedCurrent = current.value?.projectId === id
    conversations.value = conversations.value.filter(conv => conv.projectId !== id)

    // A deleted project's conversation can no longer be the active row. Select
    // an existing fallback project first; otherwise create an unowned session.
    if (deletedCurrent || !current.value || current.value.projectId === id) {
      const fallbackProject = projects.value.find(project => !project.isArchived)
        ?? projects.value.find(project => project.isDefault)
      const fallbackConversation = fallbackProject
        ? conversations.value
          .filter(conv => conv.projectId === fallbackProject.id)
          .sort((a, b) => b.updatedAt - a.updatedAt)[0]
        : undefined

      if (fallbackConversation) {
        openConversation(fallbackConversation.id)
      } else {
        newConversation(fallbackProject?.id, fallbackProject?.workspaceRoot)
      }
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
    const longer = (a?: string, b?: string) =>
      (a?.length ?? 0) >= (b?.length ?? 0) ? a : b
    const merged: ChatMessage[] = []
    for (const dbMsg of fromDb) {
      const live = inMemory.find(m => m.id === dbMsg.id)
      if (
        live
        && (live.status === 'streaming'
          || live.status === 'pending'
          || live.contentStreaming)
      ) {
        // Keep live streaming flags, but prefer longer persisted text / tool
        // snapshots after SSE gaps (live often misses lagged deltas).
        merged.push({
          ...dbMsg,
          ...live,
          content: longer(live.content, dbMsg.content) ?? '',
          reasoning: longer(live.reasoning, dbMsg.reasoning),
          rawContent: longer(live.rawContent, dbMsg.rawContent),
          thoughts: longer(live.thoughts, dbMsg.thoughts),
          toolCalls:
            (live.toolCalls?.length ?? 0) >= (dbMsg.toolCalls?.length ?? 0)
              ? live.toolCalls
              : dbMsg.toolCalls,
          attachments:
            (live.attachments?.length ?? 0) >= (dbMsg.attachments?.length ?? 0)
              ? live.attachments
              : dbMsg.attachments,
          agentTrace:
            (live.agentTrace?.length ?? 0) >= (dbMsg.agentTrace?.length ?? 0)
              ? live.agentTrace
              : dbMsg.agentTrace
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

  function applyMessagePageState(
    convId: string,
    page: {
      hasMoreOlder: boolean
      hasMoreNewer: boolean
      oldestPosition: number | null
      newestPosition: number | null
    },
    extra?: Partial<MessagePageState>
  ) {
    messagePageByConv.value = {
      ...messagePageByConv.value,
      [convId]: {
        hasMoreOlder: page.hasMoreOlder,
        hasMoreNewer: page.hasMoreNewer,
        oldestPosition: page.oldestPosition,
        newestPosition: page.newestPosition,
        loadingOlder: false,
        ...extra
      }
    }
  }

  function messagePageState(convId: string): MessagePageState | null {
    return messagePageByConv.value[convId] ?? null
  }

  function prependMessagesById(existing: ChatMessage[], older: ChatMessage[]): ChatMessage[] {
    if (older.length === 0) return existing
    const seen = new Set(existing.map(m => m.id))
    const uniqueOlder = older.filter(m => {
      if (seen.has(m.id)) return false
      seen.add(m.id)
      return true
    })
    return uniqueOlder.length ? [...uniqueOlder, ...existing] : existing
  }

  /** Copy wire-only `positions` (parallel to `page.messages`) onto the message objects. */
  function attachPagePositions(page: { messages: ChatMessage[]; positions?: number[] }): void {
    if (!page.positions || page.positions.length === 0) return
    page.messages.forEach((m, i) => {
      const pos = page.positions?.[i]
      if (pos != null) m.position = pos
    })
  }

  async function ensureMessagesLoaded(
    id: string,
    options?: { force?: boolean; silent?: boolean }
  ): Promise<boolean> {
    const convId = id.trim()
    if (!convId) return false
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      console.warn('[chat] ensureMessagesLoaded: missing conversation', convId)
      return false
    }
    const staleHydration =
      hydratedIds.value.has(convId)
      && (conv.messageCount ?? 0) > 0
      && conv.messages.length === 0
    if (!options?.force && hydratedIds.value.has(convId) && !staleHydration) return true
    if (staleHydration) {
      console.warn('[chat] ensureMessagesLoaded: stale hydration, reloading', convId)
    }
    if (
      !options?.force
      && !staleHydration
      && isConversationGenerating(convId)
      && conv.messages.length > 0
    ) {
      console.info('[chat] ensureMessagesLoaded: skip hydrating generating conversation', convId)
      return true
    }
    const existing = messageHydrationPromises.get(convId)
    if (existing) return existing

    const hydration = (async (): Promise<boolean> => {
      // silent: background SSE catch-up — avoid toggling hydrating UI (page flash).
      if (!options?.silent) {
        messagesLoadingIds.value = new Set([...messagesLoadingIds.value, convId])
      }
      try {
        // force (stream catch-up): full transcript; otherwise last N user turns.
        const page = await loadConversationMessagesPage(convId, {
          limitTurns: options?.force ? 0 : DEFAULT_MESSAGE_PAGE_TURNS
        })
        attachPagePositions(page)
        const stripped = stripWireAttachmentFields(
          page.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
        )
        const dbPersistedIds = persistedCandidateMessageIds(stripped)
        let next = stripped
        if (isImConversation(convId)) {
          next = dedupeImInboundUserMessages(convId, stripped)
        }
        if (isConversationGenerating(convId) && conv.messages.length > 0) {
          next = mergeHydratedMessages(conv.messages, next)
          addPersistedMessageIds(convId, dbPersistedIds)
          console.info(
            '[chat] ensureMessagesLoaded: merged DB rows with in-memory stream',
            convId,
            next.length
          )
        } else {
          replacePersistedMessageIds(convId, persistedCandidateMessageIds(next))
        }
        conv.messages = next
        if (typeof page.messageCount === 'number') {
          conv.messageCount = page.messageCount
        }
        applyMessagePageState(convId, page)
        if (!isConversationGenerating(convId)) {
          normalizeInterruptedAssistantStatuses([conv])
        }
        normalizeSubAgentTraces([conv])
        hydratedIds.value.add(convId)
        console.info(
          '[chat] ensureMessagesLoaded: hydrated',
          convId,
          next.length,
          'of',
          page.messageCount,
          'hasMoreOlder',
          page.hasMoreOlder
        )
        reconcileRunStateForConversation(convId)
        return true
      } catch (err) {
        console.error('[chat] ensureMessagesLoaded: load messages failed', convId, err)
        return false
      } finally {
        if (!options?.silent) {
          const next = new Set(messagesLoadingIds.value)
          next.delete(convId)
          messagesLoadingIds.value = next
        }
        messageHydrationPromises.delete(convId)
      }
    })()
    messageHydrationPromises.set(convId, hydration)
    return hydration
  }

  /** Prepend older complete user turns. Returns true when rows were added. */
  async function loadOlderMessages(id?: string): Promise<boolean> {
    const convId = (id ?? currentId.value ?? '').trim()
    if (!convId) return false
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      console.warn('[chat] loadOlderMessages: missing conversation', convId)
      return false
    }
    const state = messagePageState(convId)
    if (!state?.hasMoreOlder || state.oldestPosition == null) {
      console.info('[chat] loadOlderMessages: nothing older', convId)
      return false
    }
    // Strict serial: never start a second page while one is in flight.
    // Do not join the in-flight promise — callers must wait and retry later.
    if (state.loadingOlder || olderLoadPromises.has(convId)) {
      console.info('[chat] loadOlderMessages: busy, skip concurrent', convId)
      return false
    }

    applyMessagePageState(convId, state, { loadingOlder: true })
    const oldestAtStart = state.oldestPosition
    const load = (async (): Promise<boolean> => {
      try {
        const page = await loadConversationMessagesPage(convId, {
          limitTurns: DEFAULT_MESSAGE_PAGE_TURNS,
          beforePosition: oldestAtStart
        })
        attachPagePositions(page)
        const stripped = stripWireAttachmentFields(
          page.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
        )
        if (stripped.length === 0) {
          applyMessagePageState(convId, {
            hasMoreOlder: false,
            hasMoreNewer: state.hasMoreNewer,
            oldestPosition: oldestAtStart,
            newestPosition: state.newestPosition
          })
          return false
        }
        const beforeLen = conv.messages.length
        conv.messages = prependMessagesById(conv.messages, stripped)
        addPersistedMessageIds(convId, persistedCandidateMessageIds(stripped))
        normalizeSubAgentTraces([conv])
        // Older pages are historical. Stuck streaming/pending rows would be
        // classified as active turns and skip collapse when「默认收缩」is off
        // (hiddenCount=0 → full process, no elapsed chip).
        normalizeInterruptedAssistantStatuses([conv])
        applyMessagePageState(convId, {
          hasMoreOlder: page.hasMoreOlder,
          hasMoreNewer: state.hasMoreNewer,
          oldestPosition: page.oldestPosition ?? oldestAtStart,
          newestPosition: state.newestPosition
        })
        console.info(
          '[chat] loadOlderMessages: prepended',
          convId,
          conv.messages.length - beforeLen,
          'hasMoreOlder',
          page.hasMoreOlder
        )
        return conv.messages.length > beforeLen
      } catch (err) {
        console.error('[chat] loadOlderMessages failed', convId, err)
        applyMessagePageState(convId, state, { loadingOlder: false })
        return false
      } finally {
        olderLoadPromises.delete(convId)
        const cur = messagePageByConv.value[convId]
        if (cur?.loadingOlder) {
          messagePageByConv.value = {
            ...messagePageByConv.value,
            [convId]: { ...cur, loadingOlder: false }
          }
        }
      }
    })()
    olderLoadPromises.set(convId, load)
    return load
  }

  /**
   * Drop a stale `loadingOlder` flag when no request is in flight (e.g. after
   * WebView OOM left the button stuck on「加载中…」).
   */
  function clearStuckOlderLoading(id?: string): void {
    const convId = (id ?? currentId.value ?? '').trim()
    if (!convId) return
    const state = messagePageState(convId)
    if (!state?.loadingOlder || olderLoadPromises.has(convId)) return
    console.warn('[chat] clearStuckOlderLoading', convId)
    applyMessagePageState(convId, state, { loadingOlder: false })
  }

  /**
   * Trim the oldest in-memory messages of the *current* conversation so a very
   * long thread does not keep every loaded turn in RAM forever. Only user
   * messages not viewed within `TRIM_HISTORY_STALE_MS` (and their preceding
   * rows) are removed; the paging cursor is moved forward so scrolling back up
   * reloads them from disk via `loadOlderMessages`. Streamed / not-yet-
   * persisted rows are never cut.
   * @returns number of messages removed.
   */
  function trimConversationHistory(id: string): number {
    const convId = id.trim()
    if (!convId) return 0
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv || conv.messages.length === 0) return 0

    const messages = conv.messages
    const cut = computeHistoryTrimCutByViewedAt(
      messages,
      messageViewedAtByConv.get(convId) ?? EMPTY_VIEWED_MAP,
      Date.now(),
      TRIM_HISTORY_STALE_MS,
      TRIM_HISTORY_MIN_KEEP_TURNS
    )
    if (cut <= 0) return 0

    conv.messages = messages.slice(cut)
    // Drop viewedAt records for removed rows so the map does not grow unbounded.
    const viewed = messageViewedAtByConv.get(convId)
    if (viewed) {
      for (const msg of messages.slice(0, cut)) {
        viewed.delete(msg.id)
      }
    }
    const newFirst = conv.messages[0]
    if (newFirst?.position != null) {
      const state = messagePageByConv.value[convId]
      if (state) {
        messagePageByConv.value = {
          ...messagePageByConv.value,
          [convId]: {
            ...state,
            // The removed rows still exist in SQLite before this position, so
            // scrolling up re-fetches them (hasMoreOlder must stay true).
            oldestPosition: newFirst.position,
            hasMoreOlder: true
          }
        }
      }
    }
    console.info(
      '[chat] trimmed conversation history',
      convId,
      'removed',
      cut,
      'kept',
      conv.messages.length,
      'oldestPosition',
      newFirst?.position
    )
    return cut
  }

  /**
   * Record that a user message is currently visible in the viewport. Used by
   * the history trimmer to keep recently-viewed turns in memory.
   */
  function markUserMessageViewed(convId: string, messageId: string): void {
    const key = convId.trim()
    if (!key || !messageId) return
    let map = messageViewedAtByConv.get(key)
    if (!map) {
      map = new Map()
      messageViewedAtByConv.set(key, map)
    }
    map.set(messageId, Date.now())
  }

  /** Load a turn window around a message (sidebar FTS jump). */
  async function ensureMessagesAround(
    id: string,
    messageId: string,
    options?: { silent?: boolean }
  ): Promise<boolean> {
    const convId = id.trim()
    const targetId = messageId.trim()
    if (!convId || !targetId) return false
    const conv = conversations.value.find(c => c.id === convId)
    if (!conv) {
      console.warn('[chat] ensureMessagesAround: missing conversation', convId)
      return false
    }
    if (conv.messages.some(m => m.id === targetId) && hydratedIds.value.has(convId)) {
      console.info('[chat] ensureMessagesAround: target already loaded', convId, targetId)
      return true
    }

    const existing = messageHydrationPromises.get(convId)
    if (existing) {
      await existing
      if (conv.messages.some(m => m.id === targetId)) return true
    }

    const hydration = (async (): Promise<boolean> => {
      if (!options?.silent) {
        messagesLoadingIds.value = new Set([...messagesLoadingIds.value, convId])
      }
      try {
        const page = await loadConversationMessagesPage(convId, {
          limitTurns: DEFAULT_MESSAGE_PAGE_TURNS,
          aroundMessageId: targetId
        })
        attachPagePositions(page)
        const stripped = stripWireAttachmentFields(
          page.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
        )
        let next = stripped
        if (isImConversation(convId)) {
          next = dedupeImInboundUserMessages(convId, stripped)
        }
        if (isConversationGenerating(convId) && conv.messages.length > 0) {
          next = mergeHydratedMessages(conv.messages, next)
          addPersistedMessageIds(convId, persistedCandidateMessageIds(stripped))
        } else {
          replacePersistedMessageIds(convId, persistedCandidateMessageIds(next))
        }
        conv.messages = next
        if (typeof page.messageCount === 'number') {
          conv.messageCount = page.messageCount
        }
        applyMessagePageState(convId, page)
        if (!isConversationGenerating(convId)) {
          normalizeInterruptedAssistantStatuses([conv])
        }
        normalizeSubAgentTraces([conv])
        hydratedIds.value.add(convId)
        console.info(
          '[chat] ensureMessagesAround: hydrated',
          convId,
          targetId,
          next.length,
          'hasMoreOlder',
          page.hasMoreOlder,
          'hasMoreNewer',
          page.hasMoreNewer
        )
        reconcileRunStateForConversation(convId)
        return next.some(m => m.id === targetId)
      } catch (err) {
        console.error('[chat] ensureMessagesAround failed', convId, targetId, err)
        return false
      } finally {
        if (!options?.silent) {
          const next = new Set(messagesLoadingIds.value)
          next.delete(convId)
          messagesLoadingIds.value = next
        }
        messageHydrationPromises.delete(convId)
      }
    })()

    messageHydrationPromises.set(convId, hydration)
    return hydration
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
        sortConversationsInPlace()
      }
      nextCursor.value = page.nextCursor
      console.info('[chat] loadMoreConversations: appended', fresh.length, 'nextCursor=', page.nextCursor)
    } catch (err) {
      console.error('[chat] loadMoreConversations failed', err)
    } finally {
      loadingMoreConversations.value = false
    }
  }

  /**
   * Pinned metas sort first. Keep fetching pages until the trailing loaded row
   * is unpinned (or there is no next page), so the 置顶 section always has the
   * full pinned set even when it spans more than META_PAGE_SIZE.
   */
  async function ensureAllPinnedMetasLoaded(): Promise<void> {
    const maxPages = 100
    let pages = 0
    while (nextCursor.value && pages < maxPages) {
      const last = conversations.value[conversations.value.length - 1]
      if (!last?.isPinned) break
      pages += 1
      await loadMoreConversations()
    }
    const pinnedCount = conversations.value.filter(c => c.isPinned).length
    if (pages >= maxPages && nextCursor.value) {
      console.warn(
        '[chat] ensureAllPinnedMetasLoaded: stopped at page guard',
        maxPages,
        'pinned=',
        pinnedCount
      )
    } else {
      console.info('[chat] ensureAllPinnedMetasLoaded: pinned=', pinnedCount, 'extraPages=', pages)
    }
  }

  function toConversationMeta(c: Conversation): ConversationMeta {
    return {
      id: c.id,
      title: c.title,
      createdAt: c.createdAt,
      updatedAt: c.updatedAt,
      isPinned: !!c.isPinned,
      skillIds: c.skillIds,
      toolRoundsUsed: c.toolRoundsUsed,
      toolRoundsUsedSupervisor: c.toolRoundsUsedSupervisor,
      computerMonitorId: c.computerMonitorId,
      projectId: c.projectId,
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
    const conv = conversations.value.find(conversation => conversation.id === id)
    if (conv) syncProjectActivityInMemory(conv)
    scheduleMetaFlush()
  }

  function scheduleMetaFlush() {
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      void flushPersistMeta()
    }, 400)
  }

  async function flushPersistMeta(throwOnError = false) {
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
    try {
      await saveConversationMeta(metas)
    } catch (e) {
      // Preserve failed writes for the next flush instead of silently dropping them.
      const retry = new Set(dirtyMetaIds.value)
      for (const id of pending) retry.add(id)
      dirtyMetaIds.value = retry
      console.error('save meta error', e)
      if (throwOnError) throw e
      scheduleMetaFlush()
    }
  }

  /** P0: append client-held messages missing from DB (never deletes tool rows). */
  function persistAppend(conversationId: string) {
    const conv = conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    const [stripped] = stripEphemeralDesktopNoticesForDisk([conv])
    const hydrated = hydratedIds.value.has(conversationId)
    const persistedIds = hydrated ? persistedIdsFor(conversationId) : undefined
    const messages = messagesForPersistAppend(
      stripped.messages,
      persistedIds ? { persistedIds } : undefined
    )
    // Align the send watermark with what we treat as on-disk after a turn / trim.
    markConversationMessagesPersisted(conversationId)
    if (messages.length === 0) {
      if (hydrated) {
        console.info(
          '[chat] persistAppend incremental empty',
          conversationId,
          'of',
          stripped.messages.length
        )
      }
      return
    }
    if (hydrated) {
      console.info(
        '[chat] persistAppend incremental',
        conversationId,
        messages.length,
        'of',
        stripped.messages.length
      )
    }
    appendConversationMessages(conversationId, messages).catch(e =>
      console.error('append messages error', e)
    )
  }

  function newConversation(projectId?: string, workspaceRoot?: string): Conversation {
    // Global creation starts unowned. Project-menu creation passes an explicit
    // id and therefore remains directly bound to that project.
    const resolvedProjectId = projectId
    const project = resolvedProjectId ? projects.value.find(p => p.id === resolvedProjectId) : undefined
    const resolvedWorkspaceRoot = workspaceRoot ?? project?.workspaceRoot ?? ''
    const existingBlank = resolvedProjectId
      ? conversations.value.find(c => isBlankDesktopConversation(c) && c.projectId === resolvedProjectId)
      : undefined
    if (existingBlank) {
      existingBlank.updatedAt = Date.now()
      existingBlank.projectId = resolvedProjectId
      if (!resolvedProjectId) {
        existingBlank.pendingProjectId = undefined
        existingBlank.workspaceRoot = ''
        existingBlank.workspaceUserSet = false
        existingBlank.workspaceInheritDisabled = false
      } else if (resolvedWorkspaceRoot && !existingBlank.workspaceUserSet && !existingBlank.workspaceInheritDisabled) {
        existingBlank.workspaceRoot = resolvedWorkspaceRoot
      }
      conversations.value = [
        existingBlank,
        ...conversations.value.filter(c => c.id !== existingBlank.id)
      ]
      flushActiveComposerDraft()
      currentId.value = existingBlank.id
      loadActiveComposerDraft(existingBlank.id)
      hydratedIds.value.add(existingBlank.id)
      replacePersistedMessageIds(existingBlank.id, [])
      markMetaDirty(existingBlank.id)
      return existingBlank
    }
    const c: Conversation = {
      id: uid(),
      title: DEFAULT_CONVERSATION_TITLE,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      isPinned: false,
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: resolvedWorkspaceRoot,
      workspaceUserSet: false,
      workspaceInheritDisabled: false,
      leadAgentId: DEFAULT_LEAD_AGENT_ID,
      agentMode: 'single',
      projectId: resolvedProjectId
    }
    conversations.value.unshift(c)
    sortConversationsInPlace()
    flushActiveComposerDraft()
    currentId.value = c.id
    loadActiveComposerDraft(c.id)
    hydratedIds.value.add(c.id)
    replacePersistedMessageIds(c.id, [])
    markMetaDirty(c.id)
    return c
  }

  function syncConversationProjectWorkspace(conv: Conversation): void {
    if (!conv.projectId || conv.workspaceUserSet || conv.workspaceInheritDisabled) return
    const project = projectById(conv.projectId)
    const workspaceRoot = project?.workspaceRoot?.trim()
    if (!workspaceRoot || conv.workspaceRoot?.trim() === workspaceRoot) return
    conv.workspaceRoot = workspaceRoot
    markMetaDirty(conv.id)
  }

  /** Open a conversation from any sidebar entry while keeping project context coherent. */
  function openConversation(
    id: string,
    options?: {
      focusMessageId?: string
      focusQueryTerm?: string
      ensureShell?: { title?: string; updatedAt?: number; messageCount?: number; projectId?: string }
    }
  ): Conversation | null {
    const conv = conversations.value.find(c => c.id === id)
    if (!conv && !options?.ensureShell) return null
    selectConversation(id, options)
    const opened = conversations.value.find(c => c.id === id) ?? null
    if (opened) {
      syncConversationProjectWorkspace(opened)
      void ensureConversationProjectLoaded(opened)
    }
    return opened
  }

  async function switchProject(projectId: string): Promise<Conversation | null> {
    const project = projects.value.find(p => p.id === projectId && !p.isArchived)
    if (!project) return null

    const loaded = conversations.value
      .filter(c => c.projectId === projectId)
      .sort((a, b) => b.updatedAt - a.updatedAt)
    if (loaded.length === 0) {
      await loadProjectConversations(projectId, null)
    }
    const latest = conversations.value
      .filter(c => c.projectId === projectId)
      .sort((a, b) => b.updatedAt - a.updatedAt)[0]
    if (latest) {
      openConversation(latest.id)
      return latest
    }
    return newConversation(projectId, project.workspaceRoot)
  }

  async function loadProjectConversations(projectId: string, cursor: ConversationCursor | null) {
    const page = await loadProjectConversationMetas(projectId, cursor, 20)
    const existing = new Set(conversations.value.map(c => c.id))
    const fresh = page.items.map(metaToConversationShell).filter(c => !existing.has(c.id))
    if (fresh.length) {
      conversations.value = [...conversations.value, ...fresh]
      sortConversationsInPlace()
    }
    return page
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
    clearPersistedMessageIds(sessionId)
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
    clearPersistedMessageIds(sessionId)
    flushActiveComposerDraft()
    currentId.value = sessionId
    reconcileRunStateForConversation(sessionId)
    loadActiveComposerDraft(sessionId)
    void ensureMessagesLoaded(sessionId, { force: true })
    void refreshTaskBoard(sessionId)
    void refreshSubAgentTaskBoards(sessionId)
  }

  /**
   * Ensure a conversation shell exists in memory before select/hydrate.
   * Search can return hits outside the paginated sidebar meta list; without a
   * shell, `current` stays null and ChatView shows the empty welcome home.
   */
  function ensureConversationShell(meta: {
    id: string
    title?: string
    updatedAt?: number
    messageCount?: number
    projectId?: string
  }): Conversation {
    const id = meta.id.trim()
    const existing = conversations.value.find(c => c.id === id)
    if (existing) return existing
    const shell = metaToConversationShell({
      id,
      title: meta.title?.trim() || DEFAULT_CONVERSATION_TITLE,
      createdAt: meta.updatedAt ?? Date.now(),
      updatedAt: meta.updatedAt ?? Date.now(),
      skillIds: [],
      messageCount: meta.messageCount ?? 0,
      projectId: meta.projectId
    })
    conversations.value = [shell, ...conversations.value]
    console.info(
      '[chat] injected conversation shell for out-of-page select',
      id,
      shell.title,
      'messageCount',
      shell.messageCount
    )
    return shell
  }

  function selectConversation(
    id: string,
    options?: {
      focusMessageId?: string
      focusQueryTerm?: string
      /** When opening a search hit (or other out-of-page id), pass meta to inject a shell. */
      ensureShell?: { title?: string; updatedAt?: number; messageCount?: number; projectId?: string }
    }
  ) {
    const focusMessageId = options?.focusMessageId?.trim()
    if (focusMessageId) {
      const queryTerm = options?.focusQueryTerm?.trim() || undefined
      pendingFocusMessage.value = { conversationId: id, messageId: focusMessageId, queryTerm }
      console.info('[chat] pending focus message', id, focusMessageId)
    }
    if (options?.ensureShell || !conversations.value.some(c => c.id === id)) {
      ensureConversationShell({
        id,
        title: options?.ensureShell?.title,
        updatedAt: options?.ensureShell?.updatedAt,
        messageCount: options?.ensureShell?.messageCount,
        projectId: options?.ensureShell?.projectId
      })
    }
    const conv = conversations.value.find(c => c.id === id)
    const needsHydration = conversationNeedsMessageHydration(conv)
    if (currentId.value === id && !needsHydration) {
      touchConversation(id)
      clearConversationAwaitingView(id)
      if (focusMessageId && !(conv?.messages.some(m => m.id === focusMessageId))) {
        queueMicrotask(() => {
          if (currentId.value !== id) return
          void ensureMessagesAround(id, focusMessageId)
        })
      }
      return
    }
    // Persist the previous composer draft before switching selection.
    flushActiveComposerDraft()
    // Commit selection first so the sidebar can paint is-active before heavy UI work.
    currentId.value = id
    clearConversationAwaitingView(id)
    touchConversation(id)

    const selectedId = id
    const focusIdForHydrate = focusMessageId
    const hydrateOpts =
      needsHydration && (conv?.messageCount ?? 0) > 0 && (conv?.messages.length ?? 0) === 0
        ? { force: true as const }
        : undefined
    // Defer transcript hydrate / draft load / boards so they do not block the highlight frame.
    queueMicrotask(() => {
      if (currentId.value !== selectedId) return
      if (focusIdForHydrate) {
        void ensureMessagesAround(selectedId, focusIdForHydrate)
      } else {
        void ensureMessagesLoaded(selectedId, hydrateOpts)
      }
      reconcileRunStateForConversation(selectedId)
      loadActiveComposerDraft(selectedId)
    })
    requestAnimationFrame(() => {
      if (currentId.value !== selectedId) return
      evictIdleConversations()
      void refreshTaskBoard(selectedId)
      void refreshSubAgentTaskBoards(selectedId)
    })
  }

  function renameConversation(id: string, newTitle: string): void {
    const conv = conversations.value.find(c => c.id === id)
    if (!conv) return
    const trimmed = newTitle.trim()
    if (!trimmed || trimmed === conv.title) return
    conv.title = trimmed
    conv.updatedAt = Date.now()
    sortConversationsInPlace()
    markMetaDirty(id)
  }

  function toggleConversationPin(id: string): void {
    const conv = conversations.value.find(c => c.id === id)
    if (!conv) {
      console.warn('[chat] toggleConversationPin: conversation not found', id)
      return
    }
    conv.isPinned = !conv.isPinned
    sortConversationsInPlace()
    markMetaDirty(id)
    console.info('[chat] toggleConversationPin', id, conv.isPinned)
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
    clearConversationAwaitingView(id)
    clearPersistedMessageIds(id)
    const nextPages = { ...messagePageByConv.value }
    delete nextPages[id]
    messagePageByConv.value = nextPages
    clearOutboundQueue(id)
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
    clearLastConversationId(id)
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
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const scoped = scopedMessagesForTrace(
        r.conv.messages,
        r.msg.id,
        traceId.trim(),
        trace.agentInstanceId
      )
      for (let i = scoped.length - 1; i >= 0; i--) {
        const tc = scoped[i].toolCalls?.find(t => t.id === toolCallId)
        if (tc) return tc
      }
      return trace.session?.toolCalls?.find(t => t.id === toolCallId) ?? null
    }
    return r.msg.toolCalls?.find(t => t.id === toolCallId) ?? null
  }

  const terminalLive = createTerminalLiveManager({
    popup: terminalLivePopup,
    viewReadyToolCallId: terminalLiveViewReadyToolCallId,
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
      // Late flush after Done must not revive executing UI.
      if (target.status !== 'cancelled' && target.status !== 'error' && target.status !== 'done') {
        target.contentStreaming = true
        target.status = 'streaming'
      }
      return
    }
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const session = trace.session
      if (session) {
        session.reasoning = (session.reasoning || '') + text
        if (r.msg.status !== 'done' && r.msg.status !== 'cancelled' && r.msg.status !== 'error') {
          session.contentStreaming = true
        }
      }
      return
    }
    r.msg.reasoning = (r.msg.reasoning || '') + text
    if (r.msg.status !== 'cancelled' && r.msg.status !== 'error' && r.msg.status !== 'done') {
      r.msg.status = 'streaming'
      r.msg.contentStreaming = true
    }
  }

  function applyContentDeltaBatch(
    messageId: string,
    _traceId: string | undefined,
    _scopedMessageId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    r.msg.content += text
    // Keep text if a buffered delta lands after Done; do not flip back to streaming.
    if (r.msg.status !== 'cancelled' && r.msg.status !== 'error' && r.msg.status !== 'done') {
      r.msg.status = 'streaming'
      r.msg.contentStreaming = true
    }
  }

  setContentDeltaApplyHandler(applyContentDeltaBatch)
  setReasoningDeltaApplyHandler(applyReasoningDeltaBatch)

  function applyToolArgsDeltaBatch(
    messageId: string,
    toolCallId: string,
    traceId: string | undefined,
    scopedMessageId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
    if (target) {
      const tc = target.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.arguments += text
      return
    }
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const tc = trace.session?.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.arguments += text
      return
    }
    const tc = r.msg.toolCalls?.find(t => t.id === toolCallId)
    if (tc) tc.arguments += text
  }

  function applyToolOutputDeltaBatch(
    messageId: string,
    toolCallId: string,
    traceId: string | undefined,
    scopedMessageId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
    if (target) {
      const tc = target.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.terminalOutput = (tc.terminalOutput || '') + text
      return
    }
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const tc = trace.session?.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.terminalOutput = (tc.terminalOutput || '') + text
      return
    }
    const tc = r.msg.toolCalls?.find(t => t.id === toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + text
  }

  function applyWebSearchOutputDeltaBatch(
    messageId: string,
    toolCallId: string,
    traceId: string | undefined,
    scopedMessageId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
    if (target) {
      const tc = target.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + text
      return
    }
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const tc = trace.session?.toolCalls?.find(t => t.id === toolCallId)
      if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + text
      return
    }
    const tc = r.msg.toolCalls?.find(t => t.id === toolCallId)
    if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + text
  }

  setToolArgsDeltaApplyHandler(applyToolArgsDeltaBatch)
  setToolOutputDeltaApplyHandler(applyToolOutputDeltaBatch)
  setWebSearchOutputDeltaApplyHandler(applyWebSearchOutputDeltaBatch)

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

  watch(currentId, (id, previousId) => {
    if (id !== previousId) flushStreamDeltaBuffers()
    if (id) {
      clearConversationAwaitingView(id)
      writeLastConversationId(id)
    }
  }, { flush: 'sync' })

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

  function consumeStaleDoneAfterInterrupt(conversationId: string): boolean {
    const key = conversationId.trim()
    if (!key) return false
    const interruptAt = pendingInterruptDoneAt.get(key)
    if (interruptAt == null) return false
    pendingInterruptDoneAt.delete(key)
    const active = peekActiveTurn(key)
    // Newer turn already opened after interrupt → this Done is from the cancelled run.
    if (active && active.startedAt >= interruptAt) {
      return true
    }
    return false
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
      },
      consumeStaleDoneAfterInterrupt,
      markConversationAwaitingView
    }
  }

  function handleEventInner(e: StreamEvent) {
    dispatchStreamEvent(streamHandlerContext(), e)
  }

  async function bindProjectForFirstSend(conv: Conversation): Promise<boolean> {
    if (conv.messages.length > 0) return true

    const previous = {
      projectId: conv.projectId,
      pendingProjectId: conv.pendingProjectId,
      workspaceRoot: conv.workspaceRoot,
      workspaceUserSet: conv.workspaceUserSet,
      workspaceInheritDisabled: conv.workspaceInheritDisabled
    }
    const pendingProject = conv.pendingProjectId
      ? projects.value.find(project => project.id === conv.pendingProjectId && !project.isArchived)
      : undefined
    // A user-selected raw directory is already represented by workspaceUserSet;
    // only a truly unselected conversation inherits the default project.
    const defaultProject = !conv.projectId && !pendingProject && !conv.workspaceUserSet
      ? projects.value.find(project => project.isDefault && !project.isArchived)
      : undefined
    const resolvedProject = pendingProject ?? defaultProject

    if (resolvedProject) {
      conv.projectId = resolvedProject.id
      conv.workspaceRoot = resolvedProject.workspaceRoot
      conv.workspaceUserSet = false
      conv.workspaceInheritDisabled = false
    }
    conv.pendingProjectId = undefined
    markMetaDirty(conv.id)

    try {
      // Project/workspace ownership must reach persistence before any user row
      // can be appended or dispatched to terminal/tool execution.
      await flushPersistMeta(true)
      return true
    } catch (e) {
      Object.assign(conv, previous)
      markMetaDirty(conv.id)
      console.error('[chat] first-send project binding failed', e)
      showUiToast('项目绑定保存失败，请重试', 'error')
      return false
    }
  }

  async function sendUserMessage(content: string, attachments: ComposerAttachment[] = []) {
    if (!current.value) newConversation()
    const conv = current.value!
    const hasAttachments = attachments.length > 0
    if ((!content.trim() && !hasAttachments)) return
    // User gesture: unlock Web Audio so Done-time chime is not blocked by WKWebView.
    primeTaskCompleteAudio()
    if (conversationNeedsMessageHydration(conv)) {
      const hydrated = await ensureMessagesLoaded(conv.id, { force: true })
      if (!hydrated) {
        console.error('[chat] send blocked because message hydration failed', conv.id)
        showUiToast('历史消息加载失败，请重试', 'error')
        return
      }
    }
    const platformAuth = usePlatformAuthStore()
    try {
      await platformAuth.requireSession({ onTransient: 'error' })
    } catch (e) {
      console.error('[chat] platform session required failed', e)
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage: e instanceof Error ? e.message : platformAuth.loginHint()
      })
      return
    }
    if (platformAuth.tokenQuotaExhausted) {
      platformAuth.markTokenQuotaExhausted()
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        errorMessage: '账户余额已用尽'
      })
      return
    }
    if (!await bindProjectForFirstSend(conv)) return
    const wireAttachments = []
    for (const a of attachments) {
      const isOssVideo = a.kind === 'video' && !!a.remoteUrl?.trim()
      const contentBase64 = isOssVideo ? undefined : getComposerAttachmentContentBase64(a) ?? undefined
      const previewRaw = getComposerAttachmentDataUrl(a) ?? a.previewUrl
      const previewUrl =
        previewRaw &&
        (previewRaw.startsWith('data:') ||
          previewRaw.startsWith('http://') ||
          previewRaw.startsWith('https://'))
          ? previewRaw
          : undefined
      let storageRelPath = a.storageRelPath?.trim() || undefined
      // Composer should already persist on add; keep a last-chance save for older drafts.
      if (!isOssVideo && !storageRelPath) {
        try {
          if (contentBase64) {
            storageRelPath = await withRetries(async () =>
              await saveChatAttachment({
                conversationId: conv.id,
                attachmentId: a.id,
                contentBase64,
                fileName: a.fileName
              })
            )
          } else {
            throw new Error(`附件「${a.fileName}」尚未上传完成`)
          }
        } catch (e) {
          console.warn('[chat] saveChatAttachment failed', e)
          const errText = e instanceof Error ? e.message : String(e)
          conv.messages.push({
            id: uid(),
            role: 'assistant',
            content: '',
            status: 'error',
            createdAt: Date.now(),
            errorMessage: errText
          })
          return
        }
      }
      const { previewUrl: _p, contentBase64: _c, uploadState: _u, uploadProgress: _up, uploadError: _ue, localSourcePath: _lp, ...rest } = a
      wireAttachments.push({
        ...rest,
        ...(!storageRelPath && contentBase64 ? { contentBase64 } : {}),
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
    await interruptActiveTurn(current.value.id)
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

  function setConversationProject(projectId: string): boolean {
    const conv = current.value ?? newConversation()
    if (!conv || conv.projectId || conv.messages.length > 0) return false
    const project = projects.value.find(p => p.id === projectId && !p.isArchived)
    if (!project) return false
    conv.pendingProjectId = project.id
    conv.workspaceRoot = project.workspaceRoot
    conv.workspaceUserSet = false
    conv.workspaceInheritDisabled = false
    return true
  }

  function setConversationWorkspace(root: string) {
    if (!current.value) newConversation()
    if (!current.value) return
    if (current.value.projectId || current.value.messages.length > 0) return
    const trimmed = root.trim()
    current.value.pendingProjectId = undefined
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

  function openTerminalLivePopup(toolCallId: string) {
    terminalLive.open(toolCallId)
  }

  function resetForPlatformLogout() {
    clearStreamDeltaBuffers()
    conversations.value = []
    projectDetails.value = {}
    projectLoads.clear()
    currentId.value = null
    clearLastConversationId()
    nextCursor.value = null
    hydratedIds.value = new Set()
    messagePageByConv.value = {}
    olderLoadPromises.clear()
    persistedMessageIdsByConv.clear()
    messagesLoadingIds.value = new Set()
    messageHydrationPromises.clear()
    dirtyMetaIds.value = new Set()
    pendingFocusMessage.value = null
    if (composerDraftTimer != null) {
      window.clearTimeout(composerDraftTimer)
      composerDraftTimer = null
    }
    composerDraftByConvId.value = {}
    clearActiveComposer()
    clearAllRunStates()
    awaitingViewIds.value = new Set()
    outboundQueues.value = {}
    taskBoards.value = {}
    newConversation()
  }

  return {
    conversations, projects, currentId, current, currentOutboundQueue, isCurrentConversationHydrating, generating, activeGeneratingMessageId, contextCompressing, isConversationGenerating, isConversationAwaitingView, outboundQueueItems, outboundQueueCount, removeOutboundQueueItem, forceSendOutbound, uiToast, taskBoards,
    init, refreshProjects, loadMoreProjects, loadingMoreProjects, hasMoreProjects, projectById, ensureProjectLoaded, deleteProject, resetForPlatformLogout, newConversation, switchProject, openConversation, openCronConversation, openWebhookConversation, selectConversation, renameConversation, toggleConversationPin, deleteConversation,
    loadMoreConversations, loadProjectConversations, loadingMoreConversations, hasMoreConversations,
    ensureMessagesLoaded,
    loadOlderMessages,
    clearStuckOlderLoading,
    trimConversationHistory,
    markUserMessageViewed,
    ensureMessagesAround,
    messagePageState,
    messagePageByConv,
    pendingFocusMessage, clearPendingFocusMessage,
    sendUserMessage, stop, abortTerminalOnly, approve,
    refreshTaskBoard, refreshSubAgentTaskBoards, taskBoardForConversation, activeParentBoardDocument, activeParentBoardBinding, compactTaskBoardDocument, parentBoardsBoundToMessage,
    childBoardBindingForTrace, childBoardsForParent, lookupChildTaskBoard,
    setConversationWorkspace, setConversationProject, setConversationAgent,
    effectiveConversationLeadAgentId, effectiveConversationAgentMode,
    showUiToast,
    clearPlatformLoginErrorMessages,
    composerPrefill, prefillComposer, consumeComposerPrefill,
    composerText, composerAttachments, clearActiveComposer,
    getComposerDraft, setComposerDraft, clearComposerDraft,
    computerMonitorPickRequest, clearComputerMonitorPickRequest,
    terminalInputRequest, dismissTerminalInputModal,
    terminalLivePopup, terminalLiveViewReadyToolCallId, dismissTerminalLivePopup, openTerminalLivePopup
  }
})
