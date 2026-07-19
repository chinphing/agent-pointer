import type { AgentTrace, ChatMessage, SubAgentSessionUi, ToolCall } from '../types/chat'
import { incrementSubAgentToolStats } from './subAgentStats'
import { toolCallBaseName } from './messageTooling'

export function createEmptySubSession(): SubAgentSessionUi {
  return {
    stats: { searchCount: 0, readCount: 0 },
    collapsed: false,
    userExpanded: false,
    toolCalls: [],
    contentStreaming: true
  }
}

/** Whether the sub-agent frame should render collapsed (summary line only). */
export function isSubTraceUiCollapsed(trace: AgentTrace): boolean {
  if (trace.userExpanded) return false
  if (trace.collapsed) return true
  return trace.status === 'completed' || trace.status === 'failed'
}

/** Legacy: ensure nested session object for old persisted conversations. */
export function ensureSubTraceSession(trace: AgentTrace): SubAgentSessionUi {
  if (!trace.session) trace.session = createEmptySubSession()
  return trace.session
}

export function ensureSubTrace(
  msg: ChatMessage,
  traceId: string,
  patch?: Partial<AgentTrace>
): AgentTrace {
  msg.agentTrace = msg.agentTrace ?? []
  let trace = msg.agentTrace.find(a => a.id === traceId)
  if (!trace) {
    trace = {
      id: traceId,
      name: patch?.name ?? '',
      role: patch?.role ?? '',
      status: patch?.status ?? 'running',
      depth: patch?.depth ?? 1,
      detail: patch?.detail,
      agentInstanceId: patch?.agentInstanceId,
      computerTarget: patch?.computerTarget,
      collapsed: patch?.collapsed ?? false,
      userExpanded: patch?.userExpanded ?? false
    }
    msg.agentTrace.push(trace)
  } else if (patch) {
    const prevSession = trace.session
    Object.assign(trace, patch)
    if (prevSession && (patch.session === null || patch.session === undefined)) {
      trace.session = prevSession
    }
  }
  return trace
}

export function finalizeSubSession(trace: AgentTrace): void {
  if (trace.status === 'completed' || trace.status === 'failed') {
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
  const label = trace.name.trim() || '子任务'
  return `${label} · ${trace.status === 'running' ? '进行中' : trace.status}…`
}

export function migrateLegacyTraceUiState(trace: AgentTrace): void {
  const session = trace.session
  if (!session) return
  if (session.userExpanded) trace.userExpanded = true
  if (session.collapsed) trace.collapsed = true
}
