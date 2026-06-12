import { defineStore } from 'pinia'
import { ref, computed, watch, nextTick } from 'vue'
import {
  sendChat, cancelChat, abortTerminalCommand, approveToolCall, onStream,
  loadConversations,
  loadConversationMessages,
  saveConversationMeta,
  appendConversationMessages,
  saveChatAttachment
} from '../lib/api'
import type {
  AgentMode,
  ChatMessage,
  ComputerMonitorPickRequest,
  Conversation,
  ConversationMeta,
  ExcludedReason,
  StreamEvent,
  ToolCall,
  TaskBoardDocument
} from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
import { getTaskBoardSnapshot } from '../lib/api'
import { hasTaskBoardContent } from '../lib/taskBoard'
import { subTaskIdFromTraceId } from '../lib/subAgentStats'
import { stripWireAttachmentFields } from '../lib/messageNormalizer'
import { stripOutboundMediaMarkers } from '../lib/outboundMedia'
import { imConversationTitle, isImConversation } from '../lib/channel-labels'
import { dedupeImInboundUserMessages } from '../lib/imMessageDedupe'
import {
  getComposerAttachmentContentBase64,
  getComposerAttachmentDataUrl,
  hydrateComposerAttachments,
  releaseComposerAttachment
} from '../lib/attachmentPayloadStore'
import type { ComposerAttachment, ComposerDraft } from '../types/chat'

const TASK_BOARD_SUB_SEP = '\u{1f}ptr_sub_agent\u{1f}'
const TASK_BOARD_MAIN_TURN_SEP = '\u{1f}ptr_main_turn\u{1f}'
const TASK_BOARD_DEBOUNCE_MS = 300
const TERMINAL_LIVE_DELAY_MS = 5000
const taskBoardDebounceTimers = new Map<string, ReturnType<typeof setTimeout>>()

export interface TerminalLivePopup {
  messageId: string
  toolCallId: string
  traceId?: string
  command: string
  output: string
}

interface TerminalLiveTrack {
  messageId: string
  toolCallId: string
  traceId?: string
  command: string
  delayTimer: ReturnType<typeof setTimeout> | null
  dismissed: boolean
}

export interface ConversationTaskBoardState {
  parentByStoreKey: Record<string, TaskBoardDocument>
  parentBindings: Record<string, string>
  /** Child store key → lead assistant message id. */
  childBindings: Record<string, string>
  activeParentStoreKey: string | null
  childrenByParentStoreKey: Record<string, Record<string, TaskBoardDocument>>
}
import {
  isDiscardableEmptyAssistant,
  isEphemeralDesktopNoticeMessage,
  isGenerationCancelledMessage
} from '../lib/assistantMessageKind'
import { toolCallBaseName } from '../lib/messageTooling'
import { parseTerminalCommandFromArgs } from '../lib/terminalCommand'
import {
  ensureSubTrace,
  finalizeSubSession,
  recordSubToolSuccess
} from '../lib/subAgentSession'
import { buildCompressionNoticeContent } from '../lib/compressionMessage'
import { findLastRealUserMessage } from '../lib/messageContext'
import {
  clearReasoningDeltaBuffer,
  enqueueReasoningDelta,
  flushReasoningDeltaBuffer,
  setReasoningDeltaApplyHandler
} from '../lib/reasoningDeltaBatch'
import { useSkillsStore } from './skills'
import { useSettingsStore } from './settings'
import { usePlatformAuthStore } from './platformAuth'
import { isTauriRuntime } from '../lib/runtime'

function uid() { return Math.random().toString(36).slice(2) + Date.now().toString(36) }

function isTaskBoardTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim()
  return s === 'completed' || s === 'failed'
}

function anchorFromMainTaskBoardStoreKey(storeKey: string): string | null {
  const idx = storeKey.indexOf(TASK_BOARD_MAIN_TURN_SEP)
  if (idx < 0) return null
  const msgId = storeKey.slice(idx + TASK_BOARD_MAIN_TURN_SEP.length).trim()
  return msgId || null
}

function stripEphemeralDesktopNoticesForDisk(conversations: Conversation[]): Conversation[] {
  return conversations.map(c => ({
    ...c,
    messages: stripWireAttachmentFields(
      c.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
    )
  }))
}

function excludedContextState(reason: ExcludedReason): ChatMessage['contextState'] {
  return { included: false, excludedReason: reason }
}

function applyExcludedMessageIds(
  conv: Conversation,
  messageIds: string[],
  reason: ExcludedReason
) {
  const state = excludedContextState(reason)
  for (const id of messageIds) {
    const msg = conv.messages.find(m => m.id === id)
    if (msg) msg.contextState = state
  }
}

function insertMessageBeforeAnchor(
  conv: Conversation,
  insertBeforeMessageId: string,
  message: ChatMessage
) {
  if (conv.messages.some(m => m.id === message.id)) return
  const anchor = insertBeforeMessageId.trim()
  const idx = anchor ? conv.messages.findIndex(m => m.id === anchor) : -1
  const insertAt = idx >= 0 ? idx : conv.messages.length
  conv.messages.splice(insertAt, 0, message)
}

function childStoreKey(parentStoreKey: string, taskId: string): string {
  return `${parentStoreKey.trim()}${TASK_BOARD_SUB_SEP}${taskId.trim()}`
}

function normalizeSubAgentTraces(conversations: Conversation[]) {
  for (const conv of conversations) {
    for (const msg of conv.messages) {
      for (const trace of msg.agentTrace ?? []) {
        if ((trace.depth ?? 0) === 0) continue
        const terminal = trace.status === 'completed' || trace.status === 'failed'
        if (!terminal || !trace.session || trace.session.userExpanded) continue
        trace.session.collapsed = true
        finalizeSubSession(trace)
      }
    }
  }
}

function removeAssistantMessage(conv: Conversation, messageId: string): boolean {
  const idx = conv.messages.findIndex(m => m.id === messageId)
  if (idx < 0) return false
  conv.messages.splice(idx, 1)
  conv.updatedAt = Date.now()
  return true
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

function removeTrailingDiscardableEmptyAssistant(conv: Conversation): boolean {
  const last = conv.messages[conv.messages.length - 1]
  if (!last || !isDiscardableEmptyAssistant(last)) return false
  conv.messages.pop()
  conv.updatedAt = Date.now()
  return true
}

/** After reload or stop, assistant rows must not stay `streaming`/`pending` or action buttons never appear. */
function normalizeInterruptedAssistantStatuses(conversations: Conversation[]): void {
  for (const conv of conversations) {
    for (const m of conv.messages) {
      if (m.role !== 'assistant') continue
      if (m.status === 'streaming' || m.status === 'pending') {
        m.status = 'done'
      }
      m.contentStreaming = false
    }
  }
}

const DESKTOP_NOTICE_HIDE_MS = 5000
const desktopNoticeHideTimers = new Map<string, ReturnType<typeof setTimeout>>()

interface ConversationRunState {
  generating: boolean
  activeMessageId: string | null
}

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  const currentId = ref<string | null>(null)
  const runByConversation = ref<Record<string, ConversationRunState>>({})
  /** Ephemeral banner (e.g. computer screenshot done); not persisted. */
  const uiToast = ref<{ message: string; level: 'success' | 'warning' | 'error' } | null>(null)
  const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
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
  const terminalLivePopup = ref<TerminalLivePopup | null>(null)
  let terminalLiveTrack: TerminalLiveTrack | null = null
  let uiToastTimer: ReturnType<typeof setTimeout> | null = null
  let unlisten: (() => void) | null = null
  let saveTimer: number | null = null

  const current = computed(() =>
    conversations.value.find(c => c.id === currentId.value) || null
  )

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

  function hasInFlightToolCalls(msg: ChatMessage): boolean {
    return (
      msg.toolCalls?.some(
        t =>
          t.status === 'running' ||
          t.status === 'pending' ||
          t.status === 'pending_approval'
      ) ?? false
    )
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
    if (!msg.content.trim()) return
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
    if (lead !== GENERAL_AGENT_ID) return []
    return [...useSkillsStore().enabledIds]
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
        jobs.push(refreshTaskBoard(conversationId, taskId, msg.id))
      }
    }
    await Promise.all(jobs)
  }

  /** Child task board for a sub-agent trace (not gated by debug settings). */
  function lookupChildTaskBoard(
    convId: string | null,
    taskId: string,
    messageId?: string
  ): TaskBoardDocument | null {
    if (!convId || !taskId.trim()) return null
    const entry = taskBoards.value[convId]
    if (!entry) return null
    const tid = taskId.trim()
    for (const [parentStoreKey, group] of Object.entries(entry.childrenByParentStoreKey)) {
      const doc = group[tid]
      if (!doc || !hasTaskBoardContent(doc)) continue
      if (messageId?.trim()) {
        const storeKey = childStoreKey(parentStoreKey, tid)
        const bound = entry.childBindings?.[storeKey]
        if (bound && bound !== messageId.trim()) continue
      }
      return doc
    }
    return null
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
    persistMeta()
  }

  function normalizeImConversationTitles(list: Conversation[]): boolean {
    let changed = false
    for (const conv of list) {
      if (conv.title !== '新会话' || !isImConversation(conv.id)) continue
      const firstUser = conv.messages.find(m => m.role === 'user')
      const title = imConversationTitle(conv.id, { firstUserText: firstUser?.content })
      if (title === conv.title) continue
      conv.title = title
      changed = true
    }
    return changed
  }

  async function init() {
    const list = await loadConversations().catch(() => [])
    normalizeInterruptedAssistantStatuses(list)
    normalizeSubAgentTraces(list)
    for (const conv of list) {
      if (!conv.leadAgentId?.trim()) conv.leadAgentId = DEFAULT_LEAD_AGENT_ID
      if (!conv.agentMode?.trim()) conv.agentMode = 'single'
    }
    const imTitlesUpdated = normalizeImConversationTitles(list)
    for (const conv of list) {
      if (!isImConversation(conv.id)) continue
      conv.messages = dedupeImInboundUserMessages(conv.id, conv.messages)
    }
    conversations.value = stripEphemeralDesktopNoticesForDisk(list)
    if (imTitlesUpdated) persistMeta()
    if (list.length === 0) newConversation()
    else {
      currentId.value = list[0].id
      loadActiveComposerDraft(currentId.value)
    }
    if (!unlisten) unlisten = await onStream(handleEvent)
    if (currentId.value) {
      void refreshTaskBoard(currentId.value)
      void refreshSubAgentTaskBoards(currentId.value)
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
      leadAgentId: c.leadAgentId,
      agentMode: c.agentMode
    }
  }

  function persistMeta() {
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      const metas = JSON.parse(
        JSON.stringify(conversations.value.map(toConversationMeta))
      ) as ConversationMeta[]
      saveConversationMeta(metas).catch(e => console.error('save meta error', e))
    }, 400)
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
    const c: Conversation = {
      id: uid(),
      title: '新会话',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: '',
      leadAgentId: DEFAULT_LEAD_AGENT_ID,
      agentMode: 'single'
    }
    conversations.value.unshift(c)
    flushActiveComposerDraft()
    currentId.value = c.id
    loadActiveComposerDraft(c.id)
    persistMeta()
    return c
  }

  async function hydrateConversationMessagesFromStore(conversationId: string) {
    if (!isImConversation(conversationId)) return
    const conv = conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    try {
      const messages = await loadConversationMessages(conversationId)
      const deduped = dedupeImInboundUserMessages(conversationId, messages)
      if (deduped.length > conv.messages.length) {
        conv.messages = deduped
        conv.updatedAt = Date.now()
      }
    } catch (err) {
      console.warn('[chat] hydrate conversation messages failed', conversationId, err)
    }
  }

  function selectConversation(id: string) {
    if (currentId.value === id) return
    flushActiveComposerDraft()
    currentId.value = id
    loadActiveComposerDraft(id)
    void hydrateConversationMessagesFromStore(id)
    void refreshTaskBoard(id)
    void refreshSubAgentTaskBoards(id)
  }

  function deleteConversation(id: string) {
    const conv = conversations.value.find(c => c.id === id)
    if (conv) {
      for (const m of conv.messages) {
        const t = desktopNoticeHideTimers.get(m.id)
        if (t != null) {
          window.clearTimeout(t)
          desktopNoticeHideTimers.delete(m.id)
        }
      }
    }
    const i = conversations.value.findIndex(c => c.id === id)
    if (i >= 0) conversations.value.splice(i, 1)
    if (currentId.value === id) {
      clearComposerDraft(id)
      currentId.value = conversations.value[0]?.id || null
      if (!currentId.value) newConversation()
      else loadActiveComposerDraft(currentId.value)
    } else {
      clearComposerDraft(id)
    }
    persistMeta()
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
    traceId?: string
  ): ToolCall | null {
    const r = findMessage(messageId)
    if (!r) return null
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      return trace.session?.toolCalls?.find(t => t.id === toolCallId) ?? null
    }
    return r.msg.toolCalls?.find(t => t.id === toolCallId) ?? null
  }

  function terminalLiveKey(messageId: string, toolCallId: string, traceId?: string): string {
    return `${messageId}\u{1f}${toolCallId}\u{1f}${traceId?.trim() ?? ''}`
  }

  function clearTerminalLiveTrack() {
    if (terminalLiveTrack?.delayTimer) clearTimeout(terminalLiveTrack.delayTimer)
    terminalLiveTrack = null
    terminalLivePopup.value = null
  }

  function dismissTerminalLivePopup() {
    if (!terminalLivePopup.value) return
    terminalLivePopup.value = null
    if (terminalLiveTrack) terminalLiveTrack.dismissed = true
  }

  function syncTerminalLivePopupOutput(messageId: string, toolCallId: string, traceId?: string) {
    const popup = terminalLivePopup.value
    if (!popup) return
    if (terminalLiveKey(messageId, toolCallId, traceId) !== terminalLiveKey(
      popup.messageId,
      popup.toolCallId,
      popup.traceId
    )) return
    const tc = resolveToolCall(messageId, toolCallId, traceId)
    if (!tc) return
    terminalLivePopup.value = { ...popup, output: tc.terminalOutput ?? '' }
  }

  function beginTerminalLiveTrack(messageId: string, toolCallId: string, traceId?: string) {
    const tc = resolveToolCall(messageId, toolCallId, traceId)
    if (!tc || toolCallBaseName(tc.name) !== 'terminal') return

    const key = terminalLiveKey(messageId, toolCallId, traceId)
    if (
      terminalLiveTrack &&
      terminalLiveKey(
        terminalLiveTrack.messageId,
        terminalLiveTrack.toolCallId,
        terminalLiveTrack.traceId
      ) === key
    ) {
      return
    }

    clearTerminalLiveTrack()

    const track: TerminalLiveTrack = {
      messageId,
      toolCallId,
      traceId: traceId?.trim() || undefined,
      command: parseTerminalCommandFromArgs(tc.arguments),
      delayTimer: null,
      dismissed: false
    }
    terminalLiveTrack = track

    track.delayTimer = setTimeout(() => {
      if (!terminalLiveTrack) return
      if (
        terminalLiveKey(
          terminalLiveTrack.messageId,
          terminalLiveTrack.toolCallId,
          terminalLiveTrack.traceId
        ) !== key
      ) {
        return
      }
      const current = resolveToolCall(messageId, toolCallId, traceId)
      if (!current || current.status !== 'running') return
      if (terminalLiveTrack.dismissed) return
      terminalLivePopup.value = {
        messageId,
        toolCallId,
        traceId: traceId?.trim() || undefined,
        command: track.command,
        output: current.terminalOutput ?? ''
      }
    }, TERMINAL_LIVE_DELAY_MS)
  }

  function finishTerminalLiveTrack(messageId: string, toolCallId: string, traceId?: string) {
    if (!terminalLiveTrack) return
    if (
      terminalLiveKey(messageId, toolCallId, traceId) !== terminalLiveKey(
        terminalLiveTrack.messageId,
        terminalLiveTrack.toolCallId,
        terminalLiveTrack.traceId
      )
    ) {
      return
    }
    clearTerminalLiveTrack()
  }

  function handleTerminalToolCallStatus(
    messageId: string,
    toolCallId: string,
    status: ToolCall['status'],
    traceId?: string
  ) {
    const tc = resolveToolCall(messageId, toolCallId, traceId)
    if (!tc || toolCallBaseName(tc.name) !== 'terminal') return
    if (status === 'running') {
      beginTerminalLiveTrack(messageId, toolCallId, traceId)
      return
    }
    if (status === 'success' || status === 'failed' || status === 'rejected') {
      finishTerminalLiveTrack(messageId, toolCallId, traceId)
    }
  }

  function applyReasoningDeltaBatch(
    messageId: string,
    traceId: string | undefined,
    text: string
  ) {
    const r = findMessage(messageId)
    if (!r) return
    if (traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, traceId.trim())
      const session = trace.session!
      session.reasoning = (session.reasoning || '') + text
      session.contentStreaming = true
    } else {
      r.msg.reasoning = (r.msg.reasoning || '') + text
      r.msg.status = 'streaming'
      r.msg.contentStreaming = true
    }
  }

  setReasoningDeltaApplyHandler(applyReasoningDeltaBatch)

  function clearDesktopNoticeSchedule(messageId: string) {
    const t = desktopNoticeHideTimers.get(messageId)
    if (t != null) {
      window.clearTimeout(t)
      desktopNoticeHideTimers.delete(messageId)
    }
  }

  /** 仅从指定会话删除 `【桌面】` 注入行（用 conversationId 避免多会话下 find 错表）。 */
  function removeDesktopNoticeRow(conversationId: string, messageId: string) {
    const conv = conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    const i = conv.messages.findIndex(m => m.id === messageId)
    if (i < 0) return
    const msg = conv.messages[i]
    if (!isEphemeralDesktopNoticeMessage(msg)) return
    conv.messages.splice(i, 1)
    conv.updatedAt = Date.now()
  }

  /** 每次注入/更新文案后重置 5s 倒计时（必须在 store 里调度，避免组件未挂载时永不消失）。 */
  function scheduleDesktopNoticeRemoval(conversationId: string, messageId: string) {
    clearDesktopNoticeSchedule(messageId)
    desktopNoticeHideTimers.set(
      messageId,
      window.setTimeout(() => {
        desktopNoticeHideTimers.delete(messageId)
        removeDesktopNoticeRow(conversationId, messageId)
      }, DESKTOP_NOTICE_HIDE_MS)
    )
  }

  function handleEvent(e: StreamEvent) {
    try {
      handleEventInner(e)
    } catch (err) {
      console.error('[chat stream] handleEvent failed', err, e)
    }
  }

  function ensureTaskBoardEntry(convId: string): ConversationTaskBoardState {
    if (!taskBoards.value[convId]) {
      taskBoards.value[convId] = {
        parentByStoreKey: {},
        parentBindings: {},
        childBindings: {},
        activeParentStoreKey: null,
        childrenByParentStoreKey: {}
      }
    }
    return taskBoards.value[convId]
  }

  function applyTaskBoardDocument(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ) {
    const entry = ensureTaskBoardEntry(convId)
    if (storeKey === convId || !storeKey.includes(TASK_BOARD_SUB_SEP)) {
      if (hasTaskBoardContent(doc)) {
        entry.parentByStoreKey[storeKey] = doc
      } else {
        delete entry.parentByStoreKey[storeKey]
      }
      let resolvedAnchor = anchorMessageId?.trim() || entry.parentBindings[storeKey] || anchorFromMainTaskBoardStoreKey(storeKey) || ''
      if (!resolvedAnchor) {
        const conv = conversations.value.find(c => c.id === convId)
        const fallback = conv ? findLastRealUserMessage(conv.messages)?.id : null
        resolvedAnchor = fallback || ''
      }
      if (resolvedAnchor) {
        entry.parentBindings[storeKey] = resolvedAnchor
      }
      if (hasTaskBoardContent(doc) && !isTaskBoardTerminal(doc.meta?.status)) {
        entry.activeParentStoreKey = storeKey
      } else if (entry.activeParentStoreKey === storeKey && isTaskBoardTerminal(doc.meta?.status)) {
        entry.activeParentStoreKey = null
      }
    } else {
      const splitIdx = storeKey.lastIndexOf(TASK_BOARD_SUB_SEP)
      if (splitIdx > 0) {
        const parentStoreKey = storeKey.slice(0, splitIdx).trim()
        const taskId = storeKey.slice(splitIdx + TASK_BOARD_SUB_SEP.length).trim()
        if (!parentStoreKey || !taskId) return
        const group = entry.childrenByParentStoreKey[parentStoreKey] ?? {}
        if (hasTaskBoardContent(doc)) {
          group[taskId] = doc
        } else {
          delete group[taskId]
          delete entry.childBindings[storeKey]
        }
        if (Object.keys(group).length > 0) {
          entry.childrenByParentStoreKey[parentStoreKey] = group
        } else {
          delete entry.childrenByParentStoreKey[parentStoreKey]
        }
        const anchor = anchorMessageId?.trim()
        if (anchor) {
          entry.childBindings[storeKey] = anchor
        }
      }
    }
  }

  function applyTaskBoardDocumentDebounced(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ) {
    const timerKey = `${convId}\u{0}|${storeKey}`
    const prev = taskBoardDebounceTimers.get(timerKey)
    if (prev != null) window.clearTimeout(prev)
    taskBoardDebounceTimers.set(
      timerKey,
      window.setTimeout(() => {
        taskBoardDebounceTimers.delete(timerKey)
        applyTaskBoardDocument(convId, storeKey, doc, anchorMessageId)
      }, TASK_BOARD_DEBOUNCE_MS)
    )
  }

  async function refreshTaskBoard(conversationId: string, taskId?: string, anchorMessageId?: string) {
    try {
      const doc = await getTaskBoardSnapshot(conversationId, taskId)
      const inferredStoreKey =
        typeof (doc as TaskBoardDocument).task_id === 'string' &&
        (doc as TaskBoardDocument).task_id.startsWith('tb_')
          ? (doc as TaskBoardDocument).task_id.slice(3)
          : ''
      const storeKey = taskId?.trim()
        ? `${taskBoards.value[conversationId]?.activeParentStoreKey || conversationId}${TASK_BOARD_SUB_SEP}${taskId.trim()}`
        : inferredStoreKey || taskBoards.value[conversationId]?.activeParentStoreKey || conversationId
      applyTaskBoardDocument(conversationId, storeKey, doc as TaskBoardDocument, anchorMessageId)
    } catch (e) {
      console.warn('[task board] snapshot failed', e)
    }
  }

  function parentBoardsBoundToMessage(convId: string | null, messageId: string): Array<{ storeKey: string; document: TaskBoardDocument; isActive: boolean }> {
    if (!convId) return []
    const entry = taskBoards.value[convId]
    if (!entry) return []
    const list: Array<{ storeKey: string; document: TaskBoardDocument; isActive: boolean }> = []
    for (const [storeKey, anchor] of Object.entries(entry.parentBindings)) {
      if (anchor !== messageId) continue
      const document = entry.parentByStoreKey[storeKey]
      if (!document || !hasTaskBoardContent(document)) continue
      list.push({
        storeKey,
        document,
        isActive: entry.activeParentStoreKey === storeKey
      })
    }
    return list
  }

  function taskBoardForConversation(convId: string | null): ConversationTaskBoardState | null {
    if (!convId) return null
    return taskBoards.value[convId] ?? null
  }

  function childBoardsForParent(
    convId: string | null,
    parentStoreKey: string
  ): Record<string, TaskBoardDocument> {
    if (!convId) return {}
    const showChildren = useSettingsStore().settings.taskBoardShowChildBoards === true
    if (!showChildren) return {}
    const entry = taskBoards.value[convId]
    if (!entry) return {}
    return entry.childrenByParentStoreKey[parentStoreKey] ?? {}
  }

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
    const id = currentId.value
    if (!id) return
    setComposerDraft(id, {
      text: composerText.value,
      attachments: composerAttachments.value.map(a => ({ ...a }))
    })
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

  watch([composerText, composerAttachments], () => {
    if (composerDraftHydrating.value) return
    flushActiveComposerDraft()
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

  function handleEventInner(e: StreamEvent) {
    switch (e.kind) {
      case 'context_trim_applied': {
        ensureImConversation(e.conversationId)
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        applyExcludedMessageIds(conv, e.excludedMessageIds, 'task_board_trim')
        conv.updatedAt = Date.now()
        persistMeta()
        break
      }
      case 'context_compression_applied': {
        clearReasoningDeltaBuffer()
        ensureImConversation(e.conversationId)
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        applyExcludedMessageIds(conv, e.excludedMessageIds, 'context_compression')
        const summary = {
          ...e.summaryMessage,
          toolCalls: e.summaryMessage.toolCalls ?? undefined
        }
        insertMessageBeforeAnchor(conv, e.insertBeforeMessageId, summary)
        showUiToast(buildCompressionNoticeContent(e.compression), 'success')
        conv.updatedAt = Date.now()
        persistMeta()
        persistAppend(e.conversationId)
        break
      }
      case 'context_compressed': {
        const r = findMessage(e.messageId)
        if (!r || r.conv.id !== e.conversationId) break
        const agentId = e.compression.subAgentId
        if (agentId && r.msg.agentTrace?.length) {
          const step = r.msg.agentTrace.find(a => a.id === agentId)
          if (step) {
            const name = e.compression.subAgentName?.trim() || step.name
            step.detail = `${name}：上下文已压缩（${e.compression.droppedCount} 条 → 摘要）`
          }
        }
        showUiToast(buildCompressionNoticeContent(e.compression), 'success')
        r.conv.updatedAt = Date.now()
        break
      }
      case 'ui_toast': {
        if (e.conversationId && e.conversationId !== currentId.value) return
        const lv = e.level
        const level: 'success' | 'warning' | 'error' =
          lv === 'error' ? 'error' : lv === 'warning' ? 'warning' : 'success'
        showUiToast(e.message, level)
        break
      }
      case 'tool_rounds_exhausted': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) return
        const suffix = e.willRetryAfterCompress ? '\n\n（正在压缩较早对话摘要…）' : ''
        conv.messages.push({
          id: uid(),
          role: 'assistant',
          content: `【提示】${e.message}${suffix}`,
          status: 'done',
          createdAt: Date.now(),
          toolCalls: []
        })
        conv.updatedAt = Date.now()
        persistMeta()
        break
      }
      case 'message_start': {
        ensureImConversation(e.conversationId)
        patchRunState(e.conversationId, { generating: true, activeMessageId: e.messageId })
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        const existing = conv.messages.find(m => m.id === e.messageId)
        if (!existing) {
          conv.messages.push({
            id: e.messageId, role: 'assistant', content: '',
            status: 'streaming', contentStreaming: true, createdAt: Date.now(), toolCalls: []
          })
        } else {
          existing.status = 'streaming'
          existing.contentStreaming = true
        }
        break
      }
      case 'delta': {
        const r = findMessage(e.messageId)
        if (r) { r.msg.content += e.text; r.msg.status = 'streaming'; r.msg.contentStreaming = true }
        break
      }
      case 'raw_content_delta': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const session = trace.session!
          session.rawContent = (session.rawContent || '') + e.text
          session.contentStreaming = true
        } else {
          r.msg.rawContent = (r.msg.rawContent || '') + e.text
          r.msg.status = 'streaming'
          r.msg.contentStreaming = true
        }
        break
      }
      case 'reasoning_delta': {
        enqueueReasoningDelta(e.messageId, e.text, e.traceId)
        break
      }
      case 'assistant_json_partial': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const session = trace.session!
          session.contentStreaming = true
          if (e.thoughts != null && e.thoughts.trim() !== '') session.thoughts = e.thoughts
          if (e.toolName != null && e.toolName.trim() !== '') {
            session.toolNamePreview = e.toolName
            if (e.toolName.trim() !== 'response') delete session.responseTextDraft
          }
          if (e.responseText !== undefined) {
            const t = e.responseText ?? ''
            if (t.trim() !== '') session.responseTextDraft = t
            else delete session.responseTextDraft
          }
        } else {
          r.msg.status = 'streaming'
          r.msg.contentStreaming = true
          if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
          if (e.toolName != null && e.toolName.trim() !== '') {
            r.msg.toolNamePreview = e.toolName
            if (e.toolName.trim() !== 'response') delete r.msg.responseTextDraft
          }
          if (e.responseText !== undefined) {
            const t = e.responseText ?? ''
            if (t.trim() !== '') r.msg.responseTextDraft = t
            else delete r.msg.responseTextDraft
          }
        }
        break
      }
      case 'agent_step': {
        const r = findMessage(e.messageId)
        if (!r) return
        const depth = e.agent.depth ?? 0
        if (depth > 0) {
          const trace = ensureSubTrace(r.msg, e.agent.id, e.agent)
          trace.content = undefined
          r.msg.status = 'streaming'
          if (e.agent.status === 'completed' || e.agent.status === 'failed') {
            finalizeSubSession(trace)
          } else if (trace.session) {
            trace.session.contentStreaming = true
          }
          r.conv.updatedAt = Date.now()
        } else {
          r.msg.status = 'streaming'
          r.msg.agentId = e.agent.id
          r.msg.agentName = e.agent.name
          r.msg.agentTrace = r.msg.agentTrace || []
          const existing = r.msg.agentTrace.find(a => a.id === e.agent.id)
          if (existing) {
            const previousContent = existing.content || ''
            Object.assign(existing, e.agent)
            if (e.agent.content === undefined) existing.content = previousContent
          } else r.msg.agentTrace.push(e.agent)
        }
        break
      }
      case 'supervisor_plan': {
        const r = findMessage(e.messageId)
        if (!r || r.conv.id !== e.conversationId) break
        r.msg.status = 'streaming'
        r.msg.supervisorPlanTasks = e.tasks
        // Fallback pull: if `task_board_updated` is delayed/missed, still refresh panel right after plan appears.
        void refreshTaskBoard(e.conversationId)
        break
      }
      case 'task_board_updated': {
        if (e.conversationId) {
          applyTaskBoardDocumentDebounced(
            e.conversationId,
            e.storeKey,
            e.document as TaskBoardDocument,
            e.anchorMessageId
          )
        }
        break
      }
      case 'skills_updated': {
        const skillsStore = useSkillsStore()
        void skillsStore.load({ rescan: true })
        if (e.enabledIds !== undefined) {
          void skillsStore.setEnabledIds(e.enabledIds)
        }
        break
      }
      case 'workspace_updated': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
          conv.workspaceRoot = e.workspaceRoot
          conv.updatedAt = Date.now()
          persistMeta()
        }
        if (e.isEphemeralSandbox) {
          showUiToast(`已创建临时工作目录：${e.workspaceRoot}`, 'warning')
        }
        break
      }
      case 'computer_monitor_pick_required': {
        computerMonitorPickRequest.value = {
          conversationId: e.conversationId,
          messageId: e.messageId,
          toolCallId: e.toolCallId,
          monitors: e.monitors
        }
        break
      }
      case 'computer_monitor_updated': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
          conv.computerMonitorId = e.monitorId ?? undefined
          conv.updatedAt = Date.now()
          persistMeta()
        }
        break
      }
      case 'tool_call_start': {
        const r = findMessage(e.messageId)
        if (!r) return
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const session = trace.session!
          session.toolCalls = session.toolCalls || []
          const existing = session.toolCalls.find(t => t.id === e.toolCall.id)
          if (!existing) {
            session.toolCalls.push({ ...e.toolCall })
          } else {
            // Provider may emit a lightweight start first, then a richer start
            // (resolved method/risk/display fields) with the same tool_call_id.
            Object.assign(existing, e.toolCall)
          }
          session.contentStreaming = true
        } else {
          r.msg.status = 'streaming'
          r.msg.toolCalls = r.msg.toolCalls || []
          const existing = r.msg.toolCalls.find(t => t.id === e.toolCall.id)
          if (!existing) {
            r.msg.toolCalls.push({ ...e.toolCall })
          } else {
            // Keep a single card per tool_call_id, but refresh with latest metadata.
            Object.assign(existing, e.toolCall)
          }
        }
        break
      }
      case 'tool_call_args_delta': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.arguments += e.argsDelta
        } else {
          const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.arguments += e.argsDelta
        }
        break
      }
      case 'tool_call_status': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const session = trace.session!
          const tc = session.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) {
            tc.status = e.status
            if (e.result !== undefined) tc.result = e.result
            if (e.error !== undefined) tc.error = e.error
            if (e.durationMs !== undefined) tc.durationMs = e.durationMs
            if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
            if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
            if (e.status === 'success') recordSubToolSuccess(session, tc.name, tc.arguments)
          }
        } else {
          const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) {
            tc.status = e.status
            if (e.result !== undefined) tc.result = e.result
            if (e.error !== undefined) tc.error = e.error
            if (e.durationMs !== undefined) tc.durationMs = e.durationMs
            if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
            if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
          }
        }
        handleTerminalToolCallStatus(e.messageId, e.toolCallId, e.status, e.traceId)
        break
      }
      case 'terminal_output_delta': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
        } else {
          const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
        }
        syncTerminalLivePopupOutput(e.messageId, e.toolCallId, e.traceId)
        break
      }
      case 'web_search_output_delta': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
        } else {
          const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
        }
        break
      }
      case 'web_search_sources_ready': {
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.webSearchSources = e.sources
        } else {
          const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
          if (tc) tc.webSearchSources = e.sources
        }
        break
      }
      case 'message_end': {
        flushReasoningDeltaBuffer(e.messageId)
        const r = findMessage(e.messageId)
        if (r) {
          if (e.traceId?.trim()) {
            const trace = ensureSubTrace(r.msg, e.traceId.trim())
            if (trace.session) trace.session.contentStreaming = false
          } else {
            // 工具轮次/Supervisor 编排中间回合也会发 message_end，此时 generating 仍为 true
            r.msg.status = isConversationGenerating(r.conv.id) ? 'streaming' : 'done'
            r.msg.contentStreaming = false
            const preview = r.msg.toolNamePreview?.trim()
            const draft = r.msg.responseTextDraft?.trim()
            if (draft && preview && toolCallBaseName(preview) === 'response') {
              r.msg.content = stripOutboundMediaMarkers(draft)
            }
            delete r.msg.toolNamePreview
            if (e.content != null) r.msg.content = stripOutboundMediaMarkers(e.content)
            if (e.attachments?.length) r.msg.attachments = e.attachments
            if (e.rawContent != null) r.msg.rawContent = e.rawContent
            if (e.toolRawOutput != null) r.msg.toolRawOutput = e.toolRawOutput
            delete r.msg.responseTextDraft
            if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
            r.conv.updatedAt = Date.now()
            if (r.conv.title === '新会话') {
              const firstUser = r.conv.messages.find(m => m.role === 'user')
              if (isImConversation(r.conv.id)) {
                r.conv.title = imConversationTitle(r.conv.id, {
                  firstUserText: firstUser?.content
                })
              } else if (firstUser) {
                r.conv.title = firstUser.content.slice(0, 24) || '新会话'
              }
            }
            scheduleMaybeFinishGenerating(r.conv.id, e.messageId)
          }
        }
        persistMeta()
        break
      }
      case 'im_session_forked': {
        ensureImConversation(e.conversationId, e.title)
        const forked = conversations.value.find(c => c.id === e.conversationId)
        if (forked) {
          applySessionAgentToConversation(forked, e.leadAgentId, e.agentMode)
        }
        currentId.value = e.conversationId
        loadActiveComposerDraft(e.conversationId)
        persistMeta()
        break
      }
      case 'im_session_agent_changed': {
        ensureImConversation(e.conversationId)
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
          applySessionAgentToConversation(conv, e.leadAgentId, e.agentMode)
          persistMeta()
        }
        break
      }
      case 'user_message_attachments_updated': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        const msg = conv.messages.find(m => m.id === e.messageId)
        if (!msg) break
        msg.attachments = e.attachments
        if (e.content !== undefined) msg.content = e.content
        conv.updatedAt = Date.now()
        break
      }
      case 'injected_user_message': {
        ensureImConversation(e.conversationId)
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        const existing = conv.messages.find(m => m.id === e.messageId)
        if (existing) {
          existing.content = e.content
          if (e.attachments?.length) existing.attachments = e.attachments
        } else {
          conv.messages.push({
            id: e.messageId,
            role: 'user',
            content: e.content,
            status: 'done',
            createdAt: Date.now(),
            ...(e.attachments?.length ? { attachments: e.attachments } : {})
          })
        }
        conv.messages = dedupeImInboundUserMessages(e.conversationId, conv.messages)
        conv.updatedAt = Date.now()
        break
      }
      case 'injected_assistant_message': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        const existingRow = conv.messages.find(m => m.id === e.messageId)
        // 只允许覆盖「桌面」注入行，禁止误把流式助手气泡当成同名 id 改掉正文/thoughts
        if (
          existingRow &&
          existingRow.role === 'assistant' &&
          isEphemeralDesktopNoticeMessage(existingRow)
        ) {
          existingRow.content = e.content
          conv.updatedAt = Date.now()
          scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
          break
        }
        if (existingRow) break
        const row: ChatMessage = {
          id: e.messageId,
          role: 'assistant',
          content: e.content,
          status: 'done',
          createdAt: Date.now(),
          toolCalls: []
        }
        const last = conv.messages[conv.messages.length - 1]
        if (last?.role === 'assistant' && last.status === 'streaming') {
          conv.messages.splice(conv.messages.length - 1, 0, row)
        } else {
          conv.messages.push(row)
        }
        conv.updatedAt = Date.now()
        scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
        break
      }
      case 'injected_assistant_message_update': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        const msg = conv.messages.find(m => m.id === e.messageId)
        if (msg && msg.role === 'assistant' && isEphemeralDesktopNoticeMessage(msg)) {
          msg.content = e.content
          conv.updatedAt = Date.now()
          scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
        }
        break
      }
      case 'assistant_round_screen': {
        const r = findMessage(e.messageId)
        if (!r || r.conv.id !== e.conversationId) break
        if (r.msg.role !== 'assistant') break
        r.msg.computerRoundScreenRelPath = e.annotatedRelPath
        r.conv.updatedAt = Date.now()
        break
      }
      case 'error': {
        flushReasoningDeltaBuffer(e.messageId ?? undefined)
        const cancelled = isGenerationCancelledMessage(e.message)
        if (e.messageId) {
          const r = findMessage(e.messageId)
          if (r) {
            if (cancelled && isDiscardableEmptyAssistant(r.msg)) {
              removeAssistantMessage(r.conv, e.messageId)
            } else {
              r.msg.status = 'error'
              r.msg.errorMessage = e.message
              r.msg.contentStreaming = false
            }
            clearRunState(r.conv.id)
          } else {
            const id = currentId.value
            if (id && isConversationGenerating(id)) {
              clearRunState(id)
            }
          }
        } else if (cancelled) {
          const conv = conversations.value.find(c => c.id === currentId.value)
          if (conv) {
            removeTrailingDiscardableEmptyAssistant(conv)
            clearRunState(conv.id)
          }
        } else {
          const conv = conversations.value.find(c => c.id === currentId.value)
          if (conv) {
            conv.messages.push({
              id: uid(),
              role: 'assistant',
              content: '',
              status: 'error',
              createdAt: Date.now(),
              toolCalls: [],
              errorMessage: e.message
            })
            conv.updatedAt = Date.now()
          }
          clearAllRunStates()
        }
        persistMeta()
        break
      }
      case 'done': {
        const convId = e.conversationId?.trim() || currentId.value?.trim() || ''
        if (convId) clearRunState(convId)
        const cur = currentId.value?.trim()
        if (cur && cur !== convId && isConversationGenerating(cur)) {
          clearRunState(cur)
        }
        flushReasoningDeltaBuffer()
        if (convId) ensureImConversation(convId)
        const conv = convId ? conversations.value.find(c => c.id === convId) : undefined
        if (conv) {
          normalizeInterruptedAssistantStatuses([conv])
          removeTrailingDiscardableEmptyAssistant(conv)
          if (e.toolRoundsUsedTotal != null) conv.toolRoundsUsed = e.toolRoundsUsedTotal
          if (e.toolRoundsUsedSupervisorTotal != null) {
            conv.toolRoundsUsedSupervisor = e.toolRoundsUsedSupervisorTotal
          }
        }
        persistMeta()
        break
      }
    }
  }

  async function sendUserMessage(content: string, attachments: ComposerAttachment[] = []) {
    if (!current.value) newConversation()
    const conv = current.value!
    const hasAttachments = attachments.length > 0
    if ((!content.trim() && !hasAttachments) || isConversationGenerating(conv.id)) return
    const platformAuth = usePlatformAuthStore()
    let refreshErrorMessage: string | null = null
    if (isTauriRuntime()) {
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
            '请先登录 Pointer 账户'
        })
        return
      }
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
      const contentBase64 = getComposerAttachmentContentBase64(a) ?? undefined
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
      const { previewUrl: _p, contentBase64: _c, ...rest } = a
      wireAttachments.push({
        ...rest,
        ...(contentBase64 ? { contentBase64 } : {}),
        ...(previewUrl ? { previewUrl } : {}),
        ...(storageRelPath ? { storageRelPath } : {})
      })
    }
    const userMsg: ChatMessage = {
      id: uid(), role: 'user', content,
      status: 'done', createdAt: Date.now(),
      ...(wireAttachments.length ? { attachments: wireAttachments } : {})
    }
    conv.messages.push(userMsg)
    for (const att of attachments) {
      releaseComposerAttachment(att.id)
    }
    conv.updatedAt = Date.now()
    patchRunState(conv.id, { generating: true, activeMessageId: null })
    persistMeta()

    void refreshTaskBoard(conv.id)

    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: enabledSkillIdsForRequest(conv),
      agentMode: effectiveConversationAgentMode(conv),
      leadAgentId: effectiveConversationLeadAgentId(conv),
      toolRoundsUsed: conv.toolRoundsUsed ?? 0,
      toolRoundsUsedSupervisor: conv.toolRoundsUsedSupervisor ?? 0,
      workspaceRoot: conv.workspaceRoot ?? ''
    }).catch(err => {
      clearRunState(conv.id)
      console.error('sendChat error', err)
      conv.messages.push({
        id: uid(), role: 'assistant', content: '',
        status: 'error', createdAt: Date.now(),
        errorMessage: String(err)
      })
      persistAppend(conv.id)
    })
  }

  async function stop() {
    if (!current.value) return
    const conv = current.value
    const msgId = runStateFor(conv.id).activeMessageId
    flushReasoningDeltaBuffer(msgId ?? undefined)
    clearTerminalLiveTrack()
    await cancelChat(conv.id).catch(e => console.error(e))
    clearRunState(conv.id)
    if (msgId) {
      const row = conv.messages.find(m => m.id === msgId)
      if (row?.role === 'assistant' && (row.status === 'streaming' || row.status === 'pending')) {
        row.status = 'done'
      }
    }
    removeDiscardableAssistant(conv, msgId)
    persistMeta()
  }

  async function abortTerminalOnly() {
    if (!current.value) return
    await abortTerminalCommand(current.value.id).catch(e => console.error(e))
  }

  async function approve(toolCall: ToolCall, approved: boolean) {
    if (!current.value) return
    await approveToolCall(current.value.id, toolCall.id, approved)
      .catch(e => console.error(e))
  }

  function setConversationAgent(leadAgentId: string, agentMode: AgentMode = 'single') {
    const conv = current.value ?? newConversation()
    applySessionAgentToConversation(conv, leadAgentId, agentMode)
    persistMeta()
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
    current.value.workspaceRoot = root
    persistMeta()
  }

  function clearComputerMonitorPickRequest() {
    computerMonitorPickRequest.value = null
  }

  return {
    conversations, currentId, current, generating, activeGeneratingMessageId, uiToast, taskBoards,
    init, newConversation, selectConversation, deleteConversation,
    sendUserMessage, stop, abortTerminalOnly, approve,
    refreshTaskBoard, refreshSubAgentTaskBoards, taskBoardForConversation, parentBoardsBoundToMessage,
    childBoardsForParent, lookupChildTaskBoard,
    setConversationWorkspace, setConversationAgent,
    effectiveConversationLeadAgentId, effectiveConversationAgentMode,
    showUiToast,
    clearPlatformLoginErrorMessages,
    composerPrefill, prefillComposer, consumeComposerPrefill,
    composerText, composerAttachments, clearActiveComposer,
    getComposerDraft, setComposerDraft, clearComposerDraft,
    computerMonitorPickRequest, clearComputerMonitorPickRequest,
    terminalLivePopup, dismissTerminalLivePopup
  }
})
