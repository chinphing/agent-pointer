import type { AgentMessageBodyModel } from '../components/chat/message/assistant/AgentMessageBody.vue'
import type { AgentTrace, ChatMessage, Conversation, SubAgentToolStats, ToolCall } from '../types/chat'
import { parseAskUserArgs } from './askUser'
import { toolCallBaseName } from './messageTooling'
import {
  emptySubAgentToolStats,
  incrementSubAgentToolStats,
  subAgentIdFromTraceId
} from './subAgentStats'

import { useConversationScopedStore } from './conversationScoped'
import {
  PERF_ACTIVITY_TOOL_ARG_FORMAT,
  PERF_MS_TOOL_ARG_FORMAT,
  beginPerfActivity,
  endPerfActivity,
  record,
  renderPerfEnabled
} from './renderPerf'

export function isScopedSubMessage(msg: ChatMessage): boolean {
  return !!msg.anchorMessageId?.trim()
}

function rowMatchesTraceOrSpawn(row: ChatMessage, trace: string): boolean {
  const rowTrace = row.traceId?.trim() ?? ''
  const rowInstance = row.agentInstanceId?.trim() ?? ''
  return (!!rowTrace && rowTrace === trace) || (!!rowInstance && rowInstance === trace)
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
  if (!anchor || (!trace && !instance)) return []
  return messages.filter((m) => {
    if (!isScopedSubMessage(m) || m.anchorMessageId?.trim() !== anchor) return false
    if (instance) return (m.agentInstanceId?.trim() ?? '') === instance
    return rowMatchesTraceOrSpawn(m, trace)
  })
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

function richerSubAgentToolCall(current: ToolCall, incoming: ToolCall): ToolCall {
  const currentArgs = current.arguments ?? ''
  const incomingArgs = incoming.arguments ?? ''
  const ask = toolCallBaseName(current.name) === 'ask_user'
    || toolCallBaseName(incoming.name) === 'ask_user'
  if (ask) {
    const currentReady = parseAskUserArgs(currentArgs) != null
    const incomingReady = parseAskUserArgs(incomingArgs) != null
    if (incomingReady !== currentReady) return incomingReady ? incoming : current
  }
  if (incomingArgs.length !== currentArgs.length) {
    return incomingArgs.length > currentArgs.length ? incoming : current
  }
  const currentSummary = current.displaySummary?.length ?? 0
  const incomingSummary = incoming.displaySummary?.length ?? 0
  if (incomingSummary !== currentSummary) {
    return incomingSummary > currentSummary ? incoming : current
  }
  return current
}

/** Union tool calls from scoped rows and session. Same id keeps the copy that can render ask_user. */
export function mergeSubAgentToolCalls(
  lists: ReadonlyArray<readonly ToolCall[] | undefined>
): ToolCall[] {
  const byId = new Map<string, ToolCall>()
  const order: string[] = []
  for (const list of lists) {
    for (const tc of list ?? []) {
      const prev = byId.get(tc.id)
      if (!prev) {
        byId.set(tc.id, tc)
        order.push(tc.id)
        continue
      }
      if (prev !== tc) byId.set(tc.id, richerSubAgentToolCall(prev, tc))
    }
  }
  return order.map(id => byId.get(id)!)
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

export function computeSubAgentStatsFromMessages(messages: readonly ChatMessage[]): SubAgentToolStats {
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

/** One assistant round of a spawn: that round's own text plus that round's own tools. */
export interface SubAgentRoundBody {
  /** Scoped row id of the round — content anchor and conversation-search target. */
  messageId: string
  content: string
  contentStreaming: boolean
  toolCalls: ToolCall[]
  createdAt: number
}

function roundBodyFromMessage(msg: ChatMessage): SubAgentRoundBody {
  return {
    messageId: msg.id,
    content: isSubAgentHostStubContent(msg.content) ? '' : (msg.content ?? ''),
    contentStreaming: msg.contentStreaming === true || msg.status === 'streaming',
    toolCalls: msg.toolCalls ?? [],
    createdAt: msg.createdAt
  }
}

/**
 * Per-round bodies for the frame: one entry per assistant round, in time order.
 * Unlike {@link buildSubAgentBodyModelsFromScoped} this does **not** merge rounds,
 * so the frame can interleave「该轮 content → 该轮工具行」.
 */
export function buildSubAgentRoundsFromScoped(
  messages: ChatMessage[],
  anchorMessageId: string,
  traceId: string,
  agentInstanceId?: string
): SubAgentRoundBody[] {
  return scopedAssistantMessagesForTrace(
    messages,
    anchorMessageId,
    traceId,
    agentInstanceId
  )
    .map(roundBodyFromMessage)
    .filter(round => round.content.trim().length > 0 || round.toolCalls.length > 0)
}

/** Latest non-empty round text (final handoff wins); `fallback` covers legacy sessions. */
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
    allScopedMessages?: ChatMessage[]
    cutScopedRow?: ChatMessage | null
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
    if (args.cutScopedRow && isScopedSubMessage(args.cutScopedRow)) {
      const cutInThisSpawn = scopedMessagesForTrace(
        [args.cutScopedRow],
        args.anchorMessageId,
        args.traceId,
        args.agentInstanceId
      ).length > 0
      if (cutInThisSpawn) return true
      return false
    }
    const pool = args.allScopedMessages ?? args.messages
    const cutInOtherScoped = pool.some(
      m => isScopedSubMessage(m) && m.id === cut
    )
    if (cutInOtherScoped) return false
  }
  const anchor = state.messageId?.trim() ?? ''
  if (anchor && anchor !== args.anchorMessageId.trim()) return false
  const agent = state.subAgentId?.trim() ?? ''
  if (agent) {
    const fromTrace = subAgentIdFromTraceId(args.traceId)
    // After Phase G, AgentTrace.id is a SpawnId (no `:agent` suffix).
    if (fromTrace && agent !== fromTrace) return false
  }
  return true
}

export function latestSubAgentBodyModelFromSpawnRows(
  spawnRows: ChatMessage[],
  traceStatus?: string
): AgentMessageBodyModel | null {
  const assistants = spawnRows
    .filter(m => m.role === 'assistant')
    .sort((a, b) => a.createdAt - b.createdAt)
  return mergeScopedAssistantMessagesForDisplay(assistants, traceStatus)
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
  const store = useConversationScopedStore()
  const existing = store.findRow(conv.id, scopedMessageId)
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
  store.ensureInstance(conv.id, {
    agentInstanceId: child.agentInstanceId,
    anchorMessageId: child.anchorMessageId,
    traceId: child.traceId,
    taskId: child.taskId
  }, child)
  return child
}

export function findScopedMessage(
  conv: Conversation,
  scopedMessageId: string
): ChatMessage | undefined {
  const id = scopedMessageId.trim()
  return useConversationScopedStore().findRow(conv.id, id)
    ?? conv.messages.find(m => m.id === id)
}

/** Current streaming scoped assistant for this spawn, if any. */
export function findLiveScopedAssistant(
  conv: Conversation,
  lookup: { agentInstanceId?: string; anchorMessageId: string; traceId: string }
): ChatMessage | undefined {
  const rows = useConversationScopedStore().getRows(conv.id, lookup)
  return [...rows].reverse().find(
    m => m.role === 'assistant' && (m.status === 'streaming' || m.contentStreaming === true)
  )
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
  // Dev-only render-perf marker: this pass re-formats every tool call's arguments
  // with `JSON.stringify(JSON.parse(args), null, 2)`, which is the heaviest
  // `JSON.parse` call site on a long transcript. The marker names it in the HUD's
  // frame-gap blame; `lib/jsonParseProbe.ts` names it from the parse side. While
  // the HUD is off this is one boolean read and the body runs unchanged.
  const perf = renderPerfEnabled()
  const startedAt = perf ? performance.now() : 0
  if (perf) beginPerfActivity(PERF_ACTIVITY_TOOL_ARG_FORMAT)
  try {
    return collectToolRawArgs(messages)
  } finally {
    if (perf) {
      endPerfActivity(PERF_ACTIVITY_TOOL_ARG_FORMAT)
      record(PERF_MS_TOOL_ARG_FORMAT, performance.now() - startedAt)
    }
  }
}

/** The body of `buildToolRawArgsFromMessages`, unmarked (see the marker there). */
function collectToolRawArgs(messages: ChatMessage[]): string {
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
  if (latest.status === 'streaming' || latest.status === 'pending' || latest.contentStreaming) {
    return 'running'
  }
  return 'completed'
}

/** agent_step is the authority for a live running frame; hydrate must not stamp 失败. */
function resolveRehydratedTraceStatus(
  existing: AgentTrace | undefined,
  inferred: string,
  forTrace: ChatMessage[]
): string {
  const live = (existing?.status || '').trim()
  if (live === 'running' || live === 'streaming' || live === 'pending') {
    if (inferred === 'failed' || inferred === 'completed' || inferred === 'cancelled') {
      if (forTrace.some(m => m.status === 'streaming' || m.status === 'pending' || m.contentStreaming)) {
        return 'running'
      }
      if (inferred === 'failed') return live
    }
  }
  return inferred
}

function traceAgentId(traceId: string): string {
  const sep = traceId.lastIndexOf(':')
  return sep >= 0 ? traceId.slice(sep + 1).trim() : ''
}

/**
 * Rebuild degraded `agentTrace` index rows from persisted scoped child messages
 * (covers conversations saved before anchor `agent_trace` sync landed).
 */
export function rehydrateAgentTracesFromScopedMessages(
  conv: Conversation,
  scopedRows?: ChatMessage[]
): void {
  const scoped = scopedRows ?? conv.messages.filter(isScopedSubMessage)
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
      const inferred = inferTraceStatus(forTrace)
      const parentToolCallId = inferParentToolCallId(lead, forTrace, traceId, first)
      const existing = lead.agentTrace.find(
        t => t.id === traceId
          || t.id === agentInstanceId
          || (!!agentInstanceId && t.agentInstanceId === agentInstanceId)
      )
      const status = resolveRehydratedTraceStatus(existing, inferred, forTrace)
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
        id: agentInstanceId || traceId,
        name: first?.agentName?.trim() || agentId || '子任务',
        role: '',
        status,
        depth: first?.spawnDepth ?? 1,
        agentInstanceId,
        taskId: first?.taskId,
        agentId: agentId || undefined,
        parentToolCallId,
        detail: detail ? detail.slice(0, 160) : undefined,
        collapsed: true,
        userExpanded: false
      })
    }
    bindUnboundTracesToHosts(lead)
  }
  relocateNestedTracesToHostRows(conv, scoped)
}

/**
 * ③ 读取期归位（历史脏数据，**不改库**）。
 *
 * depth-2 档案曾被写进 **lead** 消息（记账层复用发起者缓冲的 bug），但它的宿主
 * `run_subagent` 行其实挂在某个 worker 的 scoped 行里。渲染树按**数据归属**解析宿主行
 * （见 `subAgentRenderTree` I1），lead 行上没有那张行 → 帧只能退化成「未关联」并贴在
 * 消息末尾（用户看到的错位）。
 *
 * 这里把这类档案**搬到宿主行所在的 scoped 行**（`scopedRow.agentTrace`）：只改内存、
 * 可回滚，同时覆盖未来漂移。宿主行仍在 lead 上的档案原样保留。
 *
 * @returns 被归位的档案条数（0 = 无需修复）。
 */
export function relocateNestedTracesToHostRows(
  conv: Conversation,
  scopedRows: readonly ChatMessage[]
): number {
  if (scopedRows.length === 0) return 0
  const hostRowByToolCallId = new Map<string, ChatMessage>()
  const rowById = new Map<string, ChatMessage>()
  for (const row of scopedRows) {
    const rowId = (row.id ?? '').trim()
    if (rowId && !rowById.has(rowId)) rowById.set(rowId, row)
    for (const tc of row.toolCalls ?? []) {
      const id = (tc.id ?? '').trim()
      if (!id || hostRowByToolCallId.has(id)) continue
      hostRowByToolCallId.set(id, row)
    }
  }
  if (hostRowByToolCallId.size === 0 && rowById.size === 0) return 0

  let moved = 0
  for (const lead of conv.messages) {
    if (lead.role !== 'assistant' || isScopedSubMessage(lead)) continue
    const traces = lead.agentTrace
    if (!traces?.length) continue
    const kept: AgentTrace[] = []
    let movedHere = 0
    for (const trace of traces) {
      const hostId = (trace.parentToolCallId ?? '').trim()
      // 宿主行优先（渲染判据）；宿主行缺失时退回档案自己的锚点 —— 老数据的宿主行
      // 可能被标成非 scoped，但 `anchorMessageId` 仍指着那条 worker 行。
      const anchor = (trace.anchorMessageId ?? '').trim()
      const host =
        (hostId ? hostRowByToolCallId.get(hostId) : undefined)
        ?? (anchor ? rowById.get(anchor) : undefined)
      if (!host) {
        kept.push(trace)
        continue
      }
      if (!host.agentTrace) host.agentTrace = []
      if (!host.agentTrace.some(t => t.id === trace.id)) host.agentTrace.push(trace)
      movedHere += 1
    }
    if (movedHere > 0) {
      lead.agentTrace = kept
      moved += movedHere
    }
  }
  return moved
}

function runSubagentHostCalls(lead: ChatMessage): ToolCall[] {
  return (lead.toolCalls ?? []).filter(tc => toolCallBaseName(tc.name) === 'run_subagent')
}

function inferParentToolCallId(
  lead: ChatMessage,
  children: ChatMessage[],
  traceId: string,
  first: ChatMessage | undefined
): string | undefined {
  const hosts = runSubagentHostCalls(lead)
  const taskId = (first?.taskId || children.find(c => c.taskId?.trim())?.taskId || '').trim()
  if (taskId && hosts.some(h => h.id === taskId)) return taskId
  const host = hosts.find(h => traceId === h.id || traceId.startsWith(`${h.id}:`))
  return host?.id
}

function agentInstanceIdFromHostTool(tc: ToolCall): string {
  const raw = tc.result?.trim()
  if (!raw) return ''
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return ''
    const id = parsed.agentInstanceId
    return typeof id === 'string' ? id.trim() : ''
  } catch {
    return ''
  }
}

function hostTraceStatus(tc: ToolCall): string {
  if (tc.status === 'failed' || tc.status === 'rejected') return 'failed'
  if (tc.status === 'success') return 'completed'
  if (tc.status === 'running' || tc.status === 'pending' || tc.status === 'pending_approval') {
    return 'running'
  }
  return 'completed'
}

/** Legacy rows omit parentToolCallId; pair leftover traces to leftover hosts in order. */
export function bindUnboundTracesToHosts(lead: ChatMessage): void {
  const hosts = runSubagentHostCalls(lead)
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

/**
 * Hydrate without scoped rows still needs a nestable trace under each host
 * so the process line can expand and lazy-load.
 */
export function ensureHostLinkedSubTraces(lead: ChatMessage): void {
  if (lead.role !== 'assistant') return
  bindUnboundTracesToHosts(lead)
  const hosts = runSubagentHostCalls(lead)
  if (hosts.length === 0) return
  lead.agentTrace = lead.agentTrace ?? []
  for (const host of hosts) {
    const hostId = host.id.trim()
    if (!hostId) continue
    const linked = lead.agentTrace.find(t => (t.parentToolCallId ?? '').trim() === hostId)
    if (linked) continue
    const instance = agentInstanceIdFromHostTool(host)
    if (!instance) continue
    const existing = lead.agentTrace.find(
      t => t.id === instance || (t.agentInstanceId ?? '').trim() === instance
    )
    if (existing) {
      if (!(existing.parentToolCallId ?? '').trim()) {
        existing.parentToolCallId = hostId
      }
      continue
    }
    lead.agentTrace.push({
      id: instance,
      name: '',
      role: '',
      status: hostTraceStatus(host),
      depth: 1,
      agentInstanceId: instance,
      parentToolCallId: hostId,
      collapsed: true,
      userExpanded: false
    })
  }
}
