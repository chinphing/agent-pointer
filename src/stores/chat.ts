import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  sendChat, cancelChat, abortTerminalCommand, approveToolCall, onStream,
  loadConversations, saveConversations
} from '../lib/api'
import type {
  ChatMessage,
  Conversation,
  StreamEvent,
  ToolCall,
  ContextCompressionInfo,
  TaskBoardDocument
} from '../types/chat'
import { getTaskBoardSnapshot } from '../lib/api'
import { hasTaskBoardContent } from '../lib/taskBoard'

const TASK_BOARD_SUB_SEP = '\u{1f}ptr_sub_agent\u{1f}'
const TASK_BOARD_DEBOUNCE_MS = 300
const taskBoardDebounceTimers = new Map<string, ReturnType<typeof setTimeout>>()

export interface ConversationTaskBoardState {
  parent: TaskBoardDocument | null
  children: Record<string, TaskBoardDocument>
}
import {
  isDiscardableEmptyAssistant,
  isEphemeralDesktopNoticeMessage,
  isGenerationCancelledMessage
} from '../lib/assistantMessageKind'
import { toolCallBaseName } from '../lib/messageTooling'
import {
  ensureSubTrace,
  finalizeSubSession,
  recordSubToolSuccess
} from '../lib/subAgentSession'
import { buildCompressionNoticeContent, isCompressionSummaryMessage } from '../lib/compressionMessage'
import { useSkillsStore } from './skills'
import { useSettingsStore } from './settings'

function uid() { return Math.random().toString(36).slice(2) + Date.now().toString(36) }

function stripEphemeralDesktopNoticesForDisk(conversations: Conversation[]): Conversation[] {
  return conversations.map(c => ({
    ...c,
    messages: c.messages.filter(m => !isEphemeralDesktopNoticeMessage(m))
  }))
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
    patchRunState(id, { generating: false, activeMessageId: null })
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

  async function init() {
    const list = await loadConversations().catch(() => [])
    normalizeInterruptedAssistantStatuses(list)
    conversations.value = stripEphemeralDesktopNoticesForDisk(list)
    if (list.length === 0) newConversation()
    else currentId.value = list[0].id
    if (!unlisten) unlisten = await onStream(handleEvent)
  }

  function persist() {
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      const payload = JSON.parse(
        JSON.stringify(stripEphemeralDesktopNoticesForDisk(conversations.value))
      )
      saveConversations(payload).catch(e => console.error('save error', e))
    }, 400)
  }

  function newConversation(): Conversation {
    const settingsStore = useSettingsStore()
    const defaultWorkspace = settingsStore.settings.workspaceRoot?.trim() || ''
    const c: Conversation = {
      id: uid(),
      title: '新会话',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0,
      workspaceRoot: defaultWorkspace
    }
    conversations.value.unshift(c)
    currentId.value = c.id
    persist()
    return c
  }

  function selectConversation(id: string) {
    currentId.value = id
    void refreshTaskBoard(id)
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
      currentId.value = conversations.value[0]?.id || null
      if (!currentId.value) newConversation()
    }
    persist()
  }

  function findMessage(messageId: string): { conv: Conversation; msg: ChatMessage } | null {
    for (const conv of conversations.value) {
      const msg = conv.messages.find(m => m.id === messageId)
      if (msg) return { conv, msg }
    }
    return null
  }

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
    persist()
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
      taskBoards.value[convId] = { parent: null, children: {} }
    }
    return taskBoards.value[convId]
  }

  function applyTaskBoardDocument(convId: string, storeKey: string, doc: TaskBoardDocument) {
    const entry = ensureTaskBoardEntry(convId)
    if (storeKey === convId || !storeKey.includes(TASK_BOARD_SUB_SEP)) {
      entry.parent = hasTaskBoardContent(doc) ? doc : null
    } else {
      const parts = storeKey.split(TASK_BOARD_SUB_SEP)
      const taskId = parts[parts.length - 1]?.trim()
      if (taskId) {
        if (hasTaskBoardContent(doc)) {
          entry.children[taskId] = doc
        } else {
          delete entry.children[taskId]
        }
      }
    }
  }

  function applyTaskBoardDocumentDebounced(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument
  ) {
    const timerKey = `${convId}\u{0}|${storeKey}`
    const prev = taskBoardDebounceTimers.get(timerKey)
    if (prev != null) window.clearTimeout(prev)
    taskBoardDebounceTimers.set(
      timerKey,
      window.setTimeout(() => {
        taskBoardDebounceTimers.delete(timerKey)
        applyTaskBoardDocument(convId, storeKey, doc)
      }, TASK_BOARD_DEBOUNCE_MS)
    )
  }

  async function refreshTaskBoard(conversationId: string, taskId?: string) {
    try {
      const doc = await getTaskBoardSnapshot(conversationId, taskId)
      const storeKey = taskId?.trim()
        ? `${conversationId}${TASK_BOARD_SUB_SEP}${taskId.trim()}`
        : conversationId
      applyTaskBoardDocument(conversationId, storeKey, doc as TaskBoardDocument)
    } catch (e) {
      console.warn('[task board] snapshot failed', e)
    }
  }

  function taskBoardForConversation(convId: string | null): ConversationTaskBoardState | null {
    if (!convId) return null
    return taskBoards.value[convId] ?? null
  }

  function showUiToast(message: string, level: 'success' | 'warning' | 'error') {
    uiToast.value = { message, level }
    if (uiToastTimer != null) window.clearTimeout(uiToastTimer)
    uiToastTimer = window.setTimeout(() => {
      uiToast.value = null
      uiToastTimer = null
    }, 4500)
  }

  function insertCompressionNotice(conv: Conversation, info: ContextCompressionInfo) {
    const notice: ChatMessage = {
      id: uid(),
      role: 'assistant',
      content: buildCompressionNoticeContent(info),
      status: 'done',
      createdAt: Date.now(),
      toolCalls: []
    }
    const summaryIdx = conv.messages.findIndex(m => isCompressionSummaryMessage(m))
    const insertAt = summaryIdx >= 0 ? summaryIdx + 1 : conv.messages.length
    conv.messages.splice(insertAt, 0, notice)
  }

  function handleEventInner(e: StreamEvent) {
    switch (e.kind) {
      case 'history_replaced': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) return
        conv.messages = e.messages
          .map(m => ({
            ...m,
            toolCalls: m.toolCalls ?? (m.role === 'assistant' ? [] : undefined)
          }))
          .filter(m => !isEphemeralDesktopNoticeMessage(m))
        if (e.compression) {
          insertCompressionNotice(conv, e.compression)
        }
        conv.updatedAt = Date.now()
        persist()
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
        persist()
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
        persist()
        break
      }
      case 'message_start': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) return
        patchRunState(e.conversationId, { generating: true, activeMessageId: e.messageId })
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
        const r = findMessage(e.messageId)
        if (!r) break
        if (e.traceId?.trim()) {
          const trace = ensureSubTrace(r.msg, e.traceId.trim())
          const session = trace.session!
          session.reasoning = (session.reasoning || '') + e.text
          session.contentStreaming = true
        } else {
          r.msg.reasoning = (r.msg.reasoning || '') + e.text
          r.msg.status = 'streaming'
          r.msg.contentStreaming = true
        }
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
          if (e.headline != null && e.headline.trim() !== '') session.headline = e.headline
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
          if (e.headline != null && e.headline.trim() !== '') r.msg.headline = e.headline
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
            trace.session.collapsed = false
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
        break
      }
      case 'task_board_updated': {
        if (e.conversationId) {
          applyTaskBoardDocumentDebounced(
            e.conversationId,
            e.storeKey,
            e.document as TaskBoardDocument
          )
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
          if (!session.toolCalls.find(t => t.id === e.toolCall.id)) {
            session.toolCalls.push({ ...e.toolCall })
          }
          session.contentStreaming = true
        } else {
          r.msg.status = 'streaming'
          r.msg.toolCalls = r.msg.toolCalls || []
          if (!r.msg.toolCalls.find(t => t.id === e.toolCall.id)) {
            r.msg.toolCalls.push({ ...e.toolCall })
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
            if (e.status === 'success') recordSubToolSuccess(session, tc.name)
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
        break
      }
      case 'message_end': {
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
              r.msg.content = draft
            }
            delete r.msg.toolNamePreview
            if (e.content != null) r.msg.content = e.content
            if (e.rawContent != null) r.msg.rawContent = e.rawContent
            delete r.msg.responseTextDraft
            if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
            if (e.headline != null && e.headline.trim() !== '') r.msg.headline = e.headline
            r.conv.updatedAt = Date.now()
            if (r.conv.title === '新会话') {
              const firstUser = r.conv.messages.find(m => m.role === 'user')
              if (firstUser) r.conv.title = firstUser.content.slice(0, 24) || '新会话'
            }
          }
        }
        persist()
        break
      }
      case 'injected_user_message': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) break
        if (!conv.messages.find(m => m.id === e.messageId)) {
          conv.messages.push({
            id: e.messageId,
            role: 'user',
            content: e.content,
            status: 'done',
            createdAt: Date.now()
          })
        }
        conv.updatedAt = Date.now()
        persist()
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
          persist()
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
        persist()
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
          persist()
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
        // Persist immediately so relPath survives app restart if the user quits before `done`.
        persist()
        break
      }
      case 'error': {
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
        persist()
        break
      }
      case 'done': {
        clearRunState(e.conversationId)
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
          normalizeInterruptedAssistantStatuses([conv])
          removeTrailingDiscardableEmptyAssistant(conv)
          if (e.toolRoundsUsedTotal != null) conv.toolRoundsUsed = e.toolRoundsUsedTotal
          if (e.toolRoundsUsedSupervisorTotal != null) {
            conv.toolRoundsUsedSupervisor = e.toolRoundsUsedSupervisorTotal
          }
        }
        persist()
        break
      }
    }
  }

  async function sendUserMessage(content: string) {
    if (!current.value) newConversation()
    const conv = current.value!
    if (!content.trim() || isConversationGenerating(conv.id)) return
    const { usePlatformAuthStore } = await import('./platformAuth')
    const platformAuth = usePlatformAuthStore()
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
      persist()
      return
    }
    const skills = useSkillsStore()
    const settings = useSettingsStore()
    conv.skillIds = [...skills.enabledIds]

    const userMsg: ChatMessage = {
      id: uid(), role: 'user', content,
      status: 'done', createdAt: Date.now()
    }
    conv.messages.push(userMsg)
    conv.updatedAt = Date.now()
    patchRunState(conv.id, { generating: true, activeMessageId: null })
    persist()

    void refreshTaskBoard(conv.id)

    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: conv.skillIds,
      agentMode: settings.settings.agentMode,
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
      persist()
    })
  }

  async function stop() {
    if (!current.value) return
    const conv = current.value
    const msgId = runStateFor(conv.id).activeMessageId
    await cancelChat(conv.id).catch(e => console.error(e))
    clearRunState(conv.id)
    if (msgId) {
      const row = conv.messages.find(m => m.id === msgId)
      if (row?.role === 'assistant' && (row.status === 'streaming' || row.status === 'pending')) {
        row.status = 'done'
      }
    }
    removeDiscardableAssistant(conv, msgId)
    persist()
  }

  async function abortTerminalOnly() {
    if (!current.value) return
    await abortTerminalCommand(current.value.id).catch(e => console.error(e))
  }

  async function retry() {
    if (!current.value) return
    const conv = current.value
    const settings = useSettingsStore()
    while (conv.messages.length && conv.messages[conv.messages.length - 1].role !== 'user') {
      conv.messages.pop()
    }
    const last = conv.messages[conv.messages.length - 1]
    if (!last) return
    patchRunState(conv.id, { generating: true, activeMessageId: null })
    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: conv.skillIds,
      agentMode: settings.settings.agentMode,
      toolRoundsUsed: conv.toolRoundsUsed ?? 0,
      toolRoundsUsedSupervisor: conv.toolRoundsUsedSupervisor ?? 0,
      workspaceRoot: conv.workspaceRoot ?? ''
    }).catch(err => {
      clearRunState(conv.id)
      console.error(err)
    })
  }

  async function approve(toolCall: ToolCall, approved: boolean) {
    if (!current.value) return
    await approveToolCall(current.value.id, toolCall.id, approved)
      .catch(e => console.error(e))
  }

  function undo() {
    if (!current.value || isConversationGenerating(current.value.id)) return
    const conv = current.value
    while (conv.messages.length && conv.messages[conv.messages.length - 1].role !== 'user') {
      conv.messages.pop()
    }
    if (conv.messages.length && conv.messages[conv.messages.length - 1].role === 'user') {
      conv.messages.pop()
    }
    conv.updatedAt = Date.now()
    persist()
  }

  function applyPersistedComposerDefaults() {
    const defaultWorkspace = useSettingsStore().settings.workspaceRoot?.trim() || ''
    if (!defaultWorkspace) return
    const conv = current.value
    if (conv && !conv.workspaceRoot?.trim()) {
      conv.workspaceRoot = defaultWorkspace
      persist()
    }
  }

  function setConversationWorkspace(root: string) {
    if (!current.value) newConversation()
    if (!current.value) return
    current.value.workspaceRoot = root
    persist()
    void useSettingsStore().saveAgentPreferences({ workspaceRoot: root })
  }

  return {
    conversations, currentId, current, generating, activeGeneratingMessageId, uiToast, taskBoards,
    init, newConversation, selectConversation, deleteConversation,
    sendUserMessage, stop, abortTerminalOnly, retry, approve, undo,
    refreshTaskBoard, taskBoardForConversation, setConversationWorkspace, applyPersistedComposerDefaults, showUiToast
  }
})
