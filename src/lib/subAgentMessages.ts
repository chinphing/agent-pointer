import type { AgentMessageBodyModel } from '../components/chat/message/assistant/AgentMessageBody.vue'
import type { AgentTrace, ChatMessage, Conversation, SubAgentToolStats } from '../types/chat'
import { toolCallBaseName } from './messageTooling'
import {
  emptySubAgentToolStats,
  incrementSubAgentToolStats
} from './subAgentStats'

export function isScopedSubMessage(msg: ChatMessage): boolean {
  return !!msg.anchorMessageId?.trim()
}

export function scopedMessagesForTrace(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  agentInstanceId?: string
): ChatMessage[] {
  const anchor = anchorMessageId.trim()
  const trace = traceId.trim()
  const instance = agentInstanceId?.trim()
  if (!anchor || !trace) return []
  return messages.filter(
    m =>
      isScopedSubMessage(m)
      && m.anchorMessageId?.trim() === anchor
      && m.traceId?.trim() === trace
      && (!instance || m.agentInstanceId?.trim() === instance)
  )
}

/** Assistant-only scoped rows for SubAgentFrame (excludes host stub user + tool result rows). */
export function scopedAssistantMessagesForTrace(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  agentInstanceId?: string
): ChatMessage[] {
  return scopedMessagesForTrace(messages, anchorMessageId, traceId, agentInstanceId)
    .filter(m => m.role === 'assistant')
    .sort((a, b) => a.createdAt - b.createdAt)
}

const SUB_AGENT_HOST_STUB_PREFIX = 'Begin. Your assigned task is in the system prompt'

export function isSubAgentHostStubContent(content: string | undefined): boolean {
  const text = content?.trim() ?? ''
  return text.startsWith(SUB_AGENT_HOST_STUB_PREFIX)
}

function dedupeToolCalls(messages: ChatMessage[]): ChatMessage['toolCalls'] {
  const out: NonNullable<ChatMessage['toolCalls']> = []
  const seen = new Set<string>()
  for (const msg of messages) {
    for (const tc of msg.toolCalls ?? []) {
      if (seen.has(tc.id)) continue
      seen.add(tc.id)
      out.push(tc)
    }
  }
  return out.length > 0 ? out : undefined
}

/** One display body per trace: aggregate tools across rounds; keep latest assistant text. */
function mergeScopedAssistantMessagesForDisplay(
  messages: ChatMessage[],
  traceStatus?: string
): AgentMessageBodyModel | null {
  if (messages.length === 0) return null
  const latest = messages[messages.length - 1]!
  const body = scopedMessageToBodyModel(latest, traceStatus)
  body.toolCalls = dedupeToolCalls(messages)
  const deliverable = [...messages]
    .reverse()
    .map(m => m.content?.trim())
    .find(c => c && !isSubAgentHostStubContent(c))
  if (deliverable) body.content = deliverable
  return body
}

export function computeSubAgentStatsFromMessages(messages: ChatMessage[]): SubAgentToolStats {
  const stats = emptySubAgentToolStats()
  const completedToolIds = new Set(
    messages
      .filter(m => m.role === 'tool' && m.toolCallId?.trim())
      .map(m => m.toolCallId!.trim())
  )
  for (const msg of messages.filter(m => m.role === 'assistant')) {
    for (const tc of msg.toolCalls ?? []) {
      const counted =
        tc.status === 'success'
        || (tc.status !== 'failed' && completedToolIds.has(tc.id))
      if (counted) {
        incrementSubAgentToolStats(stats, tc.name, tc.arguments)
      }
    }
  }
  return stats
}

export function subTraceHasVisibleActivityFromMessages(messages: ChatMessage[]): boolean {
  const scoped = messages.filter(m => m.role === 'assistant')
  for (const msg of scoped) {
    if (msg.thoughts?.trim()) return true
    if (msg.reasoning?.trim()) return true
    if (msg.toolNamePreview?.trim()) return true
    if ((msg.toolCalls?.length ?? 0) > 0) return true
  }
  return false
}

function scopedMessageToBodyModel(
  msg: ChatMessage,
  traceStatus?: string
): AgentMessageBodyModel {
  const failed = traceStatus === 'failed'
  const streaming = msg.status === 'streaming' || msg.contentStreaming === true
  return {
    thoughts: msg.thoughts,
    toolNamePreview: msg.toolNamePreview,
    responseTextDraft: msg.responseTextDraft,
    reasoning: msg.reasoning,
    content: msg.content,
    rawContent: msg.rawContent,
    contentStreaming: msg.contentStreaming === true,
    toolCalls: msg.toolCalls,
    status: failed ? 'error' : streaming ? 'streaming' : msg.status,
    createdAt: msg.createdAt,
    errorMessage: failed ? undefined : msg.errorMessage
  }
}

export function buildSubAgentBodyModelsFromScoped(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  traceStatus?: string,
  agentInstanceId?: string
): AgentMessageBodyModel[] {
  const scoped = scopedAssistantMessagesForTrace(
    messages,
    anchorMessageId,
    traceId,
    agentInstanceId
  )
  const merged = mergeScopedAssistantMessagesForDisplay(scoped, traceStatus)
  return merged ? [merged] : []
}

export function latestSubAgentBodyModelFromScoped(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  traceStatus?: string,
  agentInstanceId?: string
): AgentMessageBodyModel | null {
  const models = buildSubAgentBodyModelsFromScoped(
    messages,
    anchorMessageId,
    traceId,
    traceStatus,
    agentInstanceId
  )
  return models[0] ?? null
}

export function ensureScopedChildMessage(
  conv: Conversation,
  anchorMessageId: string,
  scopedMessageId: string,
  linkage: {
    traceId: string
    taskId: string
    spawnDepth: number
    agentInstanceId?: string
  }
): ChatMessage {
  const existing = conv.messages.find(m => m.id === scopedMessageId)
  if (existing) return existing
  const child: ChatMessage = {
    id: scopedMessageId,
    role: 'assistant',
    content: '',
    status: 'streaming',
    contentStreaming: true,
    createdAt: Date.now(),
    toolCalls: [],
    anchorMessageId: anchorMessageId.trim(),
    traceId: linkage.traceId.trim(),
    taskId: linkage.taskId.trim(),
    spawnDepth: linkage.spawnDepth,
    agentInstanceId: linkage.agentInstanceId?.trim() || undefined
  }
  conv.messages.push(child)
  return child
}

export function findScopedMessage(
  conv: Conversation,
  scopedMessageId: string
): ChatMessage | undefined {
  return conv.messages.find(m => m.id === scopedMessageId.trim())
}

/** Resolve the chat row stream handlers should mutate (lead or scoped child). */
export function resolveStreamWriteMessage(
  conv: Conversation,
  anchorMsg: ChatMessage,
  traceId?: string,
  scopedMessageId?: string
): ChatMessage | null {
  const scopedId = scopedMessageId?.trim()
  if (scopedId) {
    const scoped = findScopedMessage(conv, scopedId)
    if (!scoped) {
      console.warn('[stream] scoped message not found', scopedId)
      return null
    }
    return scoped
  }
  if (traceId?.trim()) return null
  return anchorMsg
}

export function buildToolRawArgsFromMessages(messages: ChatMessage[]): string {
  const chunks: string[] = []
  for (const msg of messages.filter(m => m.role === 'assistant')) {
    for (const tc of msg.toolCalls ?? []) {
      if (toolCallBaseName(tc.name) === 'response') continue
      let args = (tc.arguments ?? '').trim()
      if (args) {
        try {
          args = JSON.stringify(JSON.parse(args), null, 2)
        } catch {
          /* keep raw */
        }
      } else {
        args = '(empty)'
      }
      chunks.push(`[tool:${tc.name} id:${tc.id}]\n${args}`)
    }
  }
  return chunks.join('\n\n')
}

function inferTraceStatus(messages: ChatMessage[]): string {
  if (messages.some(m => m.status === 'streaming' || m.status === 'pending')) {
    return 'running'
  }
  if (messages.some(m => m.status === 'error')) return 'failed'
  return 'completed'
}

function traceAgentId(traceId: string): string {
  const sep = traceId.lastIndexOf(':')
  return sep >= 0 ? traceId.slice(sep + 1).trim() : ''
}

/**
 * Rebuild degraded `agentTrace` index rows from persisted scoped child messages
 * (covers conversations saved before anchor `agent_trace` sync landed).
 */
export function rehydrateAgentTracesFromScopedMessages(conv: Conversation): void {
  const scoped = conv.messages.filter(isScopedSubMessage)
  if (scoped.length === 0) return

  const byAnchor = new Map<string, ChatMessage[]>()
  for (const child of scoped) {
    const anchor = child.anchorMessageId?.trim()
    if (!anchor) continue
    const list = byAnchor.get(anchor) ?? []
    list.push(child)
    byAnchor.set(anchor, list)
  }

  for (const [anchorId, children] of byAnchor) {
    const lead = conv.messages.find(m => m.id === anchorId && m.role === 'assistant')
    if (!lead) continue
    lead.agentTrace = lead.agentTrace ?? []

    const traceIds = new Set(
      children.map(c => c.traceId?.trim()).filter((t): t is string => !!t)
    )
    for (const traceId of traceIds) {
      if (lead.agentTrace.some(t => t.id === traceId)) continue
      const allForTrace = children.filter(
        c => c.role === 'assistant' && c.traceId?.trim() === traceId
      )
      const agentInstanceId = [...allForTrace]
        .sort((a, b) => b.createdAt - a.createdAt)
        .map(message => message.agentInstanceId?.trim())
        .find((id): id is string => !!id)
      const forTrace = agentInstanceId
        ? allForTrace.filter(
            message => message.agentInstanceId?.trim() === agentInstanceId
          )
        : allForTrace
      const first = forTrace[0]
      const status = inferTraceStatus(forTrace)
      const agentId = traceAgentId(traceId)
      const detail = forTrace
        .map(m => m.content?.trim())
        .filter(c => c && !isSubAgentHostStubContent(c))
        .pop()
      const trace: AgentTrace = {
        id: traceId,
        name: first?.agentName?.trim() || agentId || '子任务',
        role: '',
        status,
        depth: first?.spawnDepth ?? 1,
        agentInstanceId,
        detail: detail ? detail.slice(0, 160) : undefined,
        collapsed: true,
        userExpanded: false
      }
      lead.agentTrace.push(trace)
    }
  }
}
