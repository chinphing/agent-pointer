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

const DESKTOP_NOTICE_HIDE_MS = 5000
const desktopNoticeHideTimers = new Map<string, ReturnType<typeof setTimeout>>()

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  const currentId = ref<string | null>(null)
  const generating = ref(false)
  /** Assistant row currently receiving stream events for the in-flight run. */
  const activeGeneratingMessageId = ref<string | null>(null)
  /** Ephemeral banner (e.g. computer screenshot done); not persisted. */
  const uiToast = ref<{ message: string; level: 'success' | 'warning' | 'error' } | null>(null)
  const taskBoards = ref<Record<string, ConversationTaskBoardState>>({})
  let uiToastTimer: ReturnType<typeof setTimeout> | null = null
  let unlisten: (() => void) | null = null
  let saveTimer: number | null = null

  const current = computed(() =>
    conversations.value.find(c => c.id === currentId.value) || null
  )

  async function init() {
    const list = await loadConversations().catch(() => [])
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
    const c: Conversation = {
      id: uid(),
      title: '新会话',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
      skillIds: [],
      toolRoundsUsed: 0,
      toolRoundsUsedSupervisor: 0
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
        activeGeneratingMessageId.value = e.messageId
        if (!conv.messages.find(m => m.id === e.messageId)) {
          conv.messages.push({
            id: e.messageId, role: 'assistant', content: '',
            status: 'streaming', createdAt: Date.now(), toolCalls: []
          })
        }
        break
      }
      case 'delta': {
        const r = findMessage(e.messageId)
        if (r) { r.msg.content += e.text; r.msg.status = 'streaming' }
        break
      }
      case 'raw_content_delta': {
        const r = findMessage(e.messageId)
        if (r) {
          r.msg.rawContent = (r.msg.rawContent || '') + e.text
          r.msg.status = 'streaming'
        }
        break
      }
      case 'reasoning_delta': {
        const r = findMessage(e.messageId)
        if (r) {
          r.msg.reasoning = (r.msg.reasoning || '') + e.text
          r.msg.status = 'streaming'
        }
        break
      }
      case 'assistant_json_partial': {
        const r = findMessage(e.messageId)
        if (!r) break
        r.msg.status = 'streaming'
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
        break
      }
      case 'agent_step': {
        const r = findMessage(e.messageId)
        if (!r) return
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
        r.msg.status = 'streaming'
        r.msg.toolCalls = r.msg.toolCalls || []
        if (!r.msg.toolCalls.find(t => t.id === e.toolCall.id)) {
          r.msg.toolCalls.push({ ...e.toolCall })
        }
        break
      }
      case 'tool_call_args_delta': {
        const r = findMessage(e.messageId)
        const tc = r?.msg.toolCalls?.find(t => t.id === e.toolCallId)
        if (tc) tc.arguments += e.argsDelta
        break
      }
      case 'tool_call_status': {
        const r = findMessage(e.messageId)
        const tc = r?.msg.toolCalls?.find(t => t.id === e.toolCallId)
        if (tc) {
          tc.status = e.status
          if (e.result !== undefined) tc.result = e.result
          if (e.error !== undefined) tc.error = e.error
          if (e.durationMs !== undefined) tc.durationMs = e.durationMs
        }
        break
      }
      case 'terminal_output_delta': {
        const r = findMessage(e.messageId)
        const tc = r?.msg.toolCalls?.find(t => t.id === e.toolCallId)
        if (tc) {
          tc.terminalOutput = (tc.terminalOutput || '') + e.output
        }
        break
      }
      case 'message_end': {
        const r = findMessage(e.messageId)
        if (r) {
          // 工具轮次/Supervisor 编排中间回合也会发 message_end，此时 generating 仍为 true
          r.msg.status = generating.value ? 'streaming' : 'done'
          // 忽略 JSON `null`：勿把正文/ thoughts 写成 null 导致界面丢字段
          if (e.content != null) r.msg.content = e.content
          if (e.rawContent != null) r.msg.rawContent = e.rawContent
          delete r.msg.responseTextDraft
          // 二次 message_end（如 response 收尾）若带空串，勿覆盖首轮已写入的 thoughts/headline
          if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
          if (e.headline != null && e.headline.trim() !== '') r.msg.headline = e.headline
          r.conv.updatedAt = Date.now()
          if (r.conv.title === '新会话') {
            const firstUser = r.conv.messages.find(m => m.role === 'user')
            if (firstUser) r.conv.title = firstUser.content.slice(0, 24) || '新会话'
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
            }
          }
        } else if (cancelled) {
          const conv = conversations.value.find(c => c.id === currentId.value)
          if (conv) removeTrailingDiscardableEmptyAssistant(conv)
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
        }
        generating.value = false
        activeGeneratingMessageId.value = null
        persist()
        break
      }
      case 'done': {
        generating.value = false
        activeGeneratingMessageId.value = null
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
          for (const m of conv.messages) {
            if (m.role === 'assistant' && (m.status === 'streaming' || m.status === 'pending')) {
              m.status = 'done'
            }
          }
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
    if (!content.trim() || generating.value) return
    const skills = useSkillsStore()
    const settings = useSettingsStore()
    conv.skillIds = [...skills.enabledIds]

    const userMsg: ChatMessage = {
      id: uid(), role: 'user', content,
      status: 'done', createdAt: Date.now()
    }
    conv.messages.push(userMsg)
    conv.updatedAt = Date.now()
    generating.value = true
    persist()

    void refreshTaskBoard(conv.id)

    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: conv.skillIds,
      agentMode: settings.settings.agentMode,
      toolRoundsUsed: conv.toolRoundsUsed ?? 0,
      toolRoundsUsedSupervisor: conv.toolRoundsUsedSupervisor ?? 0
    }).catch(err => {
      generating.value = false
      activeGeneratingMessageId.value = null
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
    const msgId = activeGeneratingMessageId.value
    await cancelChat(conv.id).catch(e => console.error(e))
    generating.value = false
    activeGeneratingMessageId.value = null
    if (removeDiscardableAssistant(conv, msgId)) persist()
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
    generating.value = true
    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: conv.skillIds,
      agentMode: settings.settings.agentMode,
      toolRoundsUsed: conv.toolRoundsUsed ?? 0,
      toolRoundsUsedSupervisor: conv.toolRoundsUsedSupervisor ?? 0
    }).catch(err => {
      generating.value = false
      activeGeneratingMessageId.value = null
      console.error(err)
    })
  }

  async function approve(toolCall: ToolCall, approved: boolean) {
    if (!current.value) return
    await approveToolCall(current.value.id, toolCall.id, approved)
      .catch(e => console.error(e))
  }

  function undo() {
    if (!current.value || generating.value) return
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

  return {
    conversations, currentId, current, generating, activeGeneratingMessageId, uiToast, taskBoards,
    init, newConversation, selectConversation, deleteConversation,
    sendUserMessage, stop, abortTerminalOnly, retry, approve, undo,
    refreshTaskBoard, taskBoardForConversation
  }
})
