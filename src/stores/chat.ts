import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  sendChat, cancelChat, approveToolCall, onStream,
  loadConversations, saveConversations
} from '../lib/api'
import type { ChatMessage, Conversation, StreamEvent, ToolCall } from '../types/chat'
import { useSkillsStore } from './skills'
import { useSettingsStore } from './settings'

function uid() { return Math.random().toString(36).slice(2) + Date.now().toString(36) }

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  const currentId = ref<string | null>(null)
  const generating = ref(false)
  let unlisten: (() => void) | null = null
  let saveTimer: number | null = null

  const current = computed(() =>
    conversations.value.find(c => c.id === currentId.value) || null
  )

  async function init() {
    const list = await loadConversations().catch(() => [])
    conversations.value = list
    if (list.length === 0) newConversation()
    else currentId.value = list[0].id
    if (!unlisten) unlisten = await onStream(handleEvent)
  }

  function persist() {
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      saveConversations(JSON.parse(JSON.stringify(conversations.value)))
        .catch(e => console.error('save error', e))
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
  }

  function deleteConversation(id: string) {
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

  function handleEvent(e: StreamEvent) {
    try {
      handleEventInner(e)
    } catch (err) {
      console.error('[chat stream] handleEvent failed', err, e)
    }
  }

  function handleEventInner(e: StreamEvent) {
    switch (e.kind) {
      case 'history_replaced': {
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (!conv) return
        conv.messages = e.messages.map(m => ({
          ...m,
          toolCalls: m.toolCalls ?? (m.role === 'assistant' ? [] : undefined)
        }))
        conv.updatedAt = Date.now()
        persist()
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
        if (r) r.msg.rawContent = (r.msg.rawContent || '') + e.text
        break
      }
      case 'reasoning_delta': {
        const settings = useSettingsStore()
        if (!settings.effectiveReasoningInMessages) break
        const r = findMessage(e.messageId)
        if (r) r.msg.reasoning = (r.msg.reasoning || '') + e.text
        break
      }
      case 'agent_step': {
        const r = findMessage(e.messageId)
        if (!r) return
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
      case 'tool_call_start': {
        const r = findMessage(e.messageId)
        if (!r) return
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
          r.msg.status = 'done'
          if (e.content !== undefined) r.msg.content = e.content
          if (e.rawContent !== undefined) r.msg.rawContent = e.rawContent
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
      case 'error': {
        if (e.messageId) {
          const r = findMessage(e.messageId)
          if (r) { r.msg.status = 'error'; r.msg.errorMessage = e.message }
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
        persist()
        break
      }
      case 'done': {
        generating.value = false
        const conv = conversations.value.find(c => c.id === e.conversationId)
        if (conv) {
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

    await sendChat({
      conversationId: conv.id,
      messages: JSON.parse(JSON.stringify(conv.messages)),
      enabledSkillIds: conv.skillIds,
      agentMode: settings.settings.agentMode,
      toolRoundsUsed: conv.toolRoundsUsed ?? 0,
      toolRoundsUsedSupervisor: conv.toolRoundsUsedSupervisor ?? 0
    }).catch(err => {
      generating.value = false
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
    await cancelChat(current.value.id).catch(e => console.error(e))
    generating.value = false
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
    conversations, currentId, current, generating,
    init, newConversation, selectConversation, deleteConversation,
    sendUserMessage, stop, retry, approve, undo
  }
})
