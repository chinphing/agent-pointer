import type { AgentMessageBodyModel } from '../components/chat/message/assistant/AgentMessageBody.vue'
import type { AgentTrace, ChatMessage, Conversation, SubAgentToolStats, ToolCall } from '../types/chat'
import { toolCallBaseName } from './messageTooling'
import { isBackgroundSubagentCall } from './toolCallDisplay'
import {
  emptySubAgentToolStats,
  incrementSubAgentToolStats,
  subAgentIdFromTraceId
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

function sortedScopedMessagesForTrace(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  agentInstanceId?: string
): ChatMessage[] {
  return scopedMessagesForTrace(messages, anchorMessageId, traceId, agentInstanceId)
    .slice()
    .sort((a, b) => a.createdAt - b.createdAt || a.id.localeCompare(b.id))
}

/**
 * Split sub-agent process bodies at the compression keep-window cut
 * (same insert-before id the parent thread uses).
 */
export function buildSubAgentBodyModelsSplitAtCut(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  insertBeforeMessageId: string | undefined,
  traceStatus?: string,
  agentInstanceId?: string
): { before: AgentMessageBodyModel[]; after: AgentMessageBodyModel[]; cutFound: boolean } {
  const all = sortedScopedMessagesForTrace(
    messages,
    anchorMessageId,
    traceId,
    agentInstanceId
  )
  const cut = insertBeforeMessageId?.trim() ?? ''
  const cutIdx = cut ? all.findIndex(m => m.id === cut) : -1
  if (cutIdx < 0) {
    const merged = mergeScopedAssistantMessagesForDisplay(
      all.filter(m => m.role === 'assistant'),
      traceStatus
    )
    return { before: merged ? [merged] : [], after: [], cutFound: false }
  }
  const beforeMerged = mergeScopedAssistantMessagesForDisplay(
    all.slice(0, cutIdx).filter(m => m.role === 'assistant'),
    traceStatus
  )
  const afterMerged = mergeScopedAssistantMessagesForDisplay(
    all.slice(cutIdx).filter(m => m.role === 'assistant'),
    traceStatus
  )
  return {
    before: beforeMerged ? [beforeMerged] : [],
    after: afterMerged ? [afterMerged] : [],
    cutFound: true
  }
}

export function subAgentFrameOwnsCompression(
  state: {
    scope?: string
    insertBeforeMessageId?: string
    messageId?: string
    subAgentId?: string
  } | null | undefined,
  args: {
    messages: ChatMessage[]
    anchorMessageId: string
    traceId: string
    agentInstanceId?: string
  }
): boolean {
  if (!state || state.scope !== 'sub_agent') return false
  const cut = state.insertBeforeMessageId?.trim() ?? ''
  const scoped = scopedMessagesForTrace(
    args.messages,
    args.anchorMessageId,
    args.traceId,
    args.agentInstanceId
  )
  if (cut) {
    if (scoped.some(m => m.id === cut)) return true
    const cutInOtherScoped = args.messages.some(
      m => isScopedSubMessage(m) && m.id === cut
    )
    if (cutInOtherScoped) return false
  }
  const anchor = state.messageId?.trim() ?? ''
  if (anchor && anchor !== args.anchorMessageId.trim()) return false
  const agent = state.subAgentId?.trim() ?? ''
  if (agent && agent !== subAgentIdFromTraceId(args.traceId)) return false
  return true
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
  const assistants = messages.filter(m => m.role === 'assistant')
  if (assistants.length === 0) return 'completed'
  const latest = [...assistants].sort((a, b) => b.createdAt - a.createdAt)[0]!
  if (latest.status === 'error') return 'failed'
  if (latest.status === 'streaming' || latest.status === 'pending') return 'running'
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
      const parentToolCallId = inferParentToolCallId(lead, forTrace, traceId, first)
      const existing = lead.agentTrace.find(t => t.id === traceId)
      if (existing) {
        existing.status = status
        if (!(existing.parentToolCallId ?? '').trim() && parentToolCallId) {
          existing.parentToolCallId = parentToolCallId
        }
        if (agentInstanceId && !existing.agentInstanceId) {
          existing.agentInstanceId = agentInstanceId
        }
        continue
      }
      const agentId = traceAgentId(traceId)
      const detail = forTrace
        .map(m => m.content?.trim())
        .filter(c => c && !isSubAgentHostStubContent(c))
        .pop()
      lead.agentTrace.push({
        id: traceId,
        name: first?.agentName?.trim() || agentId || '子任务',
        role: '',
        status,
        depth: first?.spawnDepth ?? 1,
        agentInstanceId,
        parentToolCallId,
        detail: detail ? detail.slice(0, 160) : undefined,
        collapsed: true,
        userExpanded: false
      })
    }
    bindUnboundTracesToHosts(lead)
  }
}

function backgroundHostCalls(lead: ChatMessage): ToolCall[] {
  return (lead.toolCalls ?? []).filter(isBackgroundSubagentCall)
}

function inferParentToolCallId(
  lead: ChatMessage,
  children: ChatMessage[],
  traceId: string,
  first: ChatMessage | undefined
): string | undefined {
  const hosts = backgroundHostCalls(lead)
  const taskId = (first?.taskId || children.find(c => c.taskId?.trim())?.taskId || '').trim()
  if (taskId && hosts.some(h => h.id === taskId)) return taskId
  const host = hosts.find(h => traceId === h.id || traceId.startsWith(`${h.id}:`))
  return host?.id
}

/** Legacy rows omit parentToolCallId; pair leftover traces to leftover hosts in order. */
export function bindUnboundTracesToHosts(lead: ChatMessage): void {
  const hosts = backgroundHostCalls(lead)
  if (hosts.length === 0) return
  const traces = lead.agentTrace ?? []
  for (const trace of traces) {
    if ((trace.parentToolCallId ?? '').trim()) continue
    const inferred = inferParentToolCallId(lead, [], trace.id, undefined)
    if (inferred) trace.parentToolCallId = inferred
  }
  const used = new Set(
    traces.map(t => (t.parentToolCallId ?? '').trim()).filter(Boolean)
  )
  const unboundTraces = traces.filter(t => !(t.parentToolCallId ?? '').trim())
  const unboundHosts = hosts.filter(h => !used.has(h.id))
  const n = Math.min(unboundTraces.length, unboundHosts.length)
  for (let i = 0; i < n; i += 1) {
    unboundTraces[i]!.parentToolCallId = unboundHosts[i]!.id
  }
}
