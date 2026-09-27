import { t } from '../i18n'
import type { AgentTrace, ChatMessage, SubAgentSessionUi, ToolCall } from '../types/chat'
import {
  agentInstanceIdFromTraceId,
  incrementSubAgentToolStats,
  resolveTraceAgentId,
  resolveTraceTaskId
} from './subAgentStats'
import { toolCallBaseName } from './messageTooling'

export function createEmptySubSession(): SubAgentSessionUi {
  return {
    stats: { searchCount: 0, readCount: 0 },
    collapsed: true,
    userExpanded: false,
    toolCalls: [],
    contentStreaming: true
  }
}

/** True when the sub-agent worker has finished (success, failure, or cancel). */
export function isSubAgentTraceTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim().toLowerCase()
  return s === 'completed' || s === 'failed' || s === 'cancelled' || s === 'canceled'
}

/** Whether the sub-agent frame should render collapsed (summary line only). */
export function isSubTraceUiCollapsed(trace: AgentTrace): boolean {
  // Default collapsed for running and terminal states; only user expand opens the frame.
  if (trace.userExpanded) return false
  return true
}

/** Legacy: ensure nested session object for old persisted conversations. */
export function ensureSubTraceSession(trace: AgentTrace): SubAgentSessionUi {
  if (!trace.session) trace.session = createEmptySubSession()
  return trace.session
}

export function findSubTrace(
  traces: AgentTrace[] | undefined,
  hint: string,
  instance?: string | null
): AgentTrace | undefined {
  if (!traces?.length) return undefined
  const spawn = instance?.trim() || ''
  const id = hint.trim()
  if (spawn) {
    const bySpawn = traces.find(a => a.agentInstanceId === spawn || a.id === spawn)
    if (bySpawn) return bySpawn
  }
  if (!id) return undefined
  const exact = traces.find(a => a.id === id || a.agentInstanceId === id)
  if (exact) return exact
  const embedded = agentInstanceIdFromTraceId(id)
  if (embedded) {
    const byMid = traces.find(a => a.agentInstanceId === embedded || a.id === embedded)
    if (byMid) return byMid
  }
  if (!id.includes(':')) return undefined
  const task = subTaskIdFromHint(id)
  const agent = subAgentIdFromHint(id)
  if (!task || !agent) return undefined
  const matches = traces.filter(
    t => resolveTraceTaskId(t) === task && resolveTraceAgentId(t) === agent
  )
  if (matches.length === 0) return undefined
  if (matches.length === 1) return matches[0]
  const running = [...matches].reverse().find(t => !isSubAgentTraceTerminal(t.status))
  return running ?? matches[matches.length - 1]
}

function subTaskIdFromHint(traceId: string): string {
  const i = traceId.indexOf(':')
  return i > 0 ? traceId.slice(0, i).trim() : ''
}

function subAgentIdFromHint(traceId: string): string {
  const i = traceId.lastIndexOf(':')
  return i > 0 ? traceId.slice(i + 1).trim() : ''
}

export function ensureSubTrace(
  msg: ChatMessage,
  traceId: string,
  patch?: Partial<AgentTrace>
): AgentTrace {
  if (!msg.agentTrace) msg.agentTrace = []
  const spawn =
    patch?.agentInstanceId?.trim()
    || patch?.id?.trim()
    || agentInstanceIdFromTraceId(traceId)
    || ''
  let trace = findSubTrace(msg.agentTrace, traceId, spawn || undefined)
  if (!trace) {
    const newId = spawn || traceId.trim()
    trace = {
      id: newId,
      name: patch?.name ?? '',
      role: patch?.role ?? '',
      status: patch?.status ?? 'running',
      depth: patch?.depth ?? 1,
      detail: patch?.detail,
      agentInstanceId: spawn || patch?.agentInstanceId,
      taskId: patch?.taskId,
      agentId: patch?.agentId,
      computerTarget: patch?.computerTarget,
      parentToolCallId: patch?.parentToolCallId,
      collapsed: patch?.collapsed ?? true,
      userExpanded: patch?.userExpanded ?? false
    }
    msg.agentTrace.push(trace)
  } else if (patch) {
    const prevSession = trace.session
    const keepExpanded = trace.userExpanded === true
    const keepParentToolCallId = (trace.parentToolCallId ?? '').trim()
    const keepId = trace.id
    Object.assign(trace, patch)
    if (spawn && (!trace.agentInstanceId?.trim() || trace.agentInstanceId === spawn)) {
      trace.agentInstanceId = spawn
      if (!trace.id.trim() || (trace.id.includes(':') && trace.id !== spawn)) {
        trace.id = spawn
      }
    } else if (!trace.id.trim()) {
      trace.id = keepId
    }
    if (prevSession && (patch.session === null || patch.session === undefined)) {
      trace.session = prevSession
    }
    // Later patches (e.g. sub_message_start) may omit parentToolCallId; keep linkage.
    if (!(trace.parentToolCallId ?? '').trim() && keepParentToolCallId) {
      trace.parentToolCallId = keepParentToolCallId
    }
    // Stream agent_step always carries userExpanded=false; do not wipe a manual expand.
    if (keepExpanded) {
      trace.userExpanded = true
      trace.collapsed = false
    }
  }
  return trace
}

export function finalizeSubSession(trace: AgentTrace): void {
  if (isSubAgentTraceTerminal(trace.status)) {
    if (!trace.userExpanded) trace.collapsed = true
  }
}

export function toggleSubTraceExpanded(trace: AgentTrace): void {
  if (isSubTraceUiCollapsed(trace)) {
    trace.userExpanded = true
    trace.collapsed = false
  } else {
    trace.userExpanded = false
    trace.collapsed = true
  }
}

export function subTracesForMessage(msg: ChatMessage): AgentTrace[] {
  return (msg.agentTrace ?? []).filter(t => (t.depth ?? 0) > 0)
}

/** Sub-agent frames nested under a specific parent `run_subagent` tool row. */
export function subTracesForParentToolCall(
  traces: AgentTrace[],
  toolCallId: string
): AgentTrace[] {
  const id = toolCallId.trim()
  if (!id) return []
  return traces.filter(t => (t.parentToolCallId ?? '').trim() === id)
}

/**
 * Traces without a matching parent tool row (legacy data, or tool card hidden).
 * Keep rendering these after the lead body so they are not lost.
 */
export function orphanSubTraces(
  traces: AgentTrace[],
  toolCallIds: Iterable<string>
): AgentTrace[] {
  const ids = new Set(
    [...toolCallIds].map(id => id.trim()).filter(Boolean)
  )
  return traces.filter(t => {
    const parent = (t.parentToolCallId ?? '').trim()
    return !parent || !ids.has(parent)
  })
}

export function sessionToolCalls(session: SubAgentSessionUi): ToolCall[] {
  return session.toolCalls ?? []
}

export function recordSubToolSuccess(
  session: SubAgentSessionUi,
  toolName: string,
  argsJson?: string
): void {
  incrementSubAgentToolStats(session.stats, toolName, argsJson)
}

export function isSubResponseToolName(toolName: string | undefined): boolean {
  const preview = toolName?.trim()
  return !!preview && toolCallBaseName(preview) === 'response'
}

/** Legacy: visible activity from nested session (pre-scoped-message conversations). */
export function subTraceHasVisibleActivity(trace: AgentTrace): boolean {
  const s = trace.session
  if (!s) return false
  if (s.thoughts?.trim()) return true
  if (s.reasoning?.trim()) return true
  if (s.toolNamePreview?.trim()) return true
  if ((s.toolCalls?.length ?? 0) > 0) return true
  return false
}

export function runningSubTraceSummaryLine(trace: AgentTrace): string {
  return trace.name.trim() || t('agent.ui.subtask')
}

export function migrateLegacyTraceUiState(trace: AgentTrace): void {
  const session = trace.session
  if (!session) return
  if (session.userExpanded) trace.userExpanded = true
  if (session.collapsed) trace.collapsed = true
}
