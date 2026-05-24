import type { AgentTrace, ChatMessage, SubAgentSessionUi, ToolCall } from '../types/chat'
import {
  emptySubAgentToolStats,
  formatSubAgentSummaryLine,
  incrementSubAgentToolStats,
  subAgentStatusLabel
} from './subAgentStats'
import { toolCallBaseName } from './messageTooling'

export function createEmptySubSession(): SubAgentSessionUi {
  return {
    stats: emptySubAgentToolStats(),
    collapsed: false,
    userExpanded: false,
    toolCalls: [],
    contentStreaming: true
  }
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
      detail: patch?.detail
    }
    msg.agentTrace.push(trace)
  } else if (patch) {
    const prevSession = trace.session
    Object.assign(trace, patch)
    if (prevSession) trace.session = prevSession
  }
  if (!trace.session) trace.session = createEmptySubSession()
  return trace
}

export function finalizeSubSession(trace: AgentTrace): void {
  const session = trace.session
  if (!session) return
  session.contentStreaming = false
  if (trace.status === 'completed' || trace.status === 'failed') {
    const agentId = trace.id.includes(':') ? trace.id.slice(trace.id.lastIndexOf(':') + 1) : ''
    const keepExpanded = agentId === 'research'
    if (!session.userExpanded && !keepExpanded) session.collapsed = true
    session.summaryLine = formatSubAgentSummaryLine(trace.name, trace.status, session.stats)
  }
}

export function subTracesForMessage(msg: ChatMessage): AgentTrace[] {
  return (msg.agentTrace ?? []).filter(t => (t.depth ?? 0) > 0)
}

export function sessionToolCalls(session: SubAgentSessionUi): ToolCall[] {
  return session.toolCalls ?? []
}

export function recordSubToolSuccess(session: SubAgentSessionUi, toolName: string): void {
  incrementSubAgentToolStats(session.stats, toolName)
}

export function isSubResponseToolName(toolName: string | undefined): boolean {
  const preview = toolName?.trim()
  return !!preview && toolCallBaseName(preview) === 'response'
}

export function subTraceHasVisibleActivity(trace: AgentTrace): boolean {
  const s = trace.session
  if (!s) return false
  if (s.thoughts?.trim()) return true
  if (s.headline?.trim()) return true
  if (s.toolNamePreview?.trim()) return true
  if ((s.toolCalls?.length ?? 0) > 0) return true
  return false
}

export function runningSubTraceSummaryLine(trace: AgentTrace): string {
  const label = trace.name.trim() || '子任务'
  return `${label} · ${subAgentStatusLabel(trace.status)}…`
}
