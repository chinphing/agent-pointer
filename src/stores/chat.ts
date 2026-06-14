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
  StreamEvent,
  ToolCall,
  TaskBoardDocument
} from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
import { getTaskBoardSnapshot } from '../lib/api'
import { subTaskIdFromTraceId } from '../lib/subAgentStats'
import { stripWireAttachmentFields } from '../lib/messageNormalizer'
import {
  clearReasoningDeltaBuffer,
  flushReasoningDeltaBuffer,
  setReasoningDeltaApplyHandler
} from '../lib/reasoningDeltaBatch'
import { imConversationTitle, isImConversation } from '../lib/channel-labels'
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
  finalizeSubSession
} from '../lib/subAgentSession'
import { useSkillsStore } from './skills'
import { useSettingsStore } from './settings'
import { usePlatformAuthStore } from './platformAuth'
import { isTauriRuntime } from '../lib/runtime'
import { dispatchStreamEvent, type StreamHandlerContext } from './chat/streamHandlers/dispatch'
import {
  normalizeInterruptedAssistantStatuses,
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
  const terminalLivePopup = ref<TerminalLivePopup | null>(null)
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
    return taskBoardMgr.lookupChildTaskBoard(convId, taskId, messageId)
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
      workspaceUserSet: c.workspaceUserSet,
      workspaceInheritDisabled: c.workspaceInheritDisabled,
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
    // Inherit workspaceRoot from the last active conversation.
    const lastWorkspace = conversations.value.length > 0
      ? (conversations.value[0].workspaceRoot ?? '')
      : '';
    const c: Conversation = {
      id: uid(),
      title: '新会话',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: lastWorkspace,
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
        desktopNotice.clearSchedule(m.id)
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

  const terminalLive = createTerminalLiveManager({
    popup: terminalLivePopup,
    resolveToolCall
  })

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
    applyTaskBoardDocumentDebounced,
    refreshTaskBoard,
    parentBoardsBoundToMessage,
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

  function streamHandlerContext(): StreamHandlerContext {
    return {
      conversations,
      currentId,
      computerMonitorPickRequest,
      ensureImConversation,
      findMessage,
      persistMeta,
      persistAppend,
      showUiToast,
      patchRunState,
      clearRunState,
      clearAllRunStates,
      isConversationGenerating,
      hasInFlightToolCalls,
      scheduleMaybeFinishGenerating,
      applyTaskBoardDocumentDebounced,
      refreshTaskBoard,
      handleTerminalToolCallStatus: terminalLive.handleToolCallStatus,
      syncTerminalLivePopupOutput: terminalLive.syncPopupOutput,
      scheduleDesktopNoticeRemoval,
      applySessionAgentToConversation,
      loadActiveComposerDraft
    }
  }

  function handleEventInner(e: StreamEvent) {
    dispatchStreamEvent(streamHandlerContext(), e)
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
    const trimmed = root.trim()
    current.value.workspaceRoot = root
    if (trimmed.length > 0) {
      current.value.workspaceUserSet = true
      current.value.workspaceInheritDisabled = false
    } else {
      current.value.workspaceUserSet = false
      current.value.workspaceInheritDisabled = true
    }
    persistMeta()
  }

  function clearComputerMonitorPickRequest() {
    computerMonitorPickRequest.value = null
  }

  function dismissTerminalLivePopup() {
    terminalLive.dismiss()
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
