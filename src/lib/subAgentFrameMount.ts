import type { AgentTrace, ChatMessage, ToolCall } from '../types/chat'
import { computeSubAgentStatsFromMessages } from './subAgentMessages'
import { isSubAgentTraceTerminal, isSubResponseToolName, isSubTraceUiCollapsed } from './subAgentSession'
import {
  emptySubAgentToolStats,
  resolveCollapsedSubAgentView,
  resolveTraceAgentId,
  type CollapsedSubAgentView
} from './subAgentStats'
import { isBackgroundSubagentCall } from './toolCallDisplay'
import { visibleToolCalls } from './messageTooling'

/** Idle delay before a terminal collapsed frame downgrades to a lightweight stub. */
export const TERMINAL_COLLAPSED_STUB_IDLE_MS = 60_000

/**
 * Hydrated traces: already terminal on first mount → stub immediately.
 * Everything else terminal+collapsed (in-session finish, user re-collapse) → 60s grace.
 */
export function shouldStubTerminalImmediately(input: {
  initialized: boolean
  status: string
}): boolean {
  return !input.initialized && isSubAgentTraceTerminal(input.status)
}

export function shouldKeepFullSubAgentFrame(trace: AgentTrace): boolean {
  if (!isSubAgentTraceTerminal(trace.status)) return true
  if (trace.userExpanded === true) return true
  return false
}

export function shouldRenderTerminalSubAgentStub(
  trace: AgentTrace,
  stubIdleElapsed: boolean
): boolean {
  if (!stubIdleElapsed) return false
  if (!isSubAgentTraceTerminal(trace.status)) return false
  if (trace.userExpanded === true) return false
  if (!isSubTraceUiCollapsed(trace)) return false
  return true
}

export function terminalSubAgentHasInnerTools(
  scopedTraceMessages: readonly ChatMessage[]
): boolean {
  const seen = new Set<string>()
  for (const msg of scopedTraceMessages) {
    for (const tc of visibleToolCalls(msg.toolCalls)) {
      if (isSubResponseToolName(tc.name)) continue
      if (tc.id?.trim()) seen.add(tc.id.trim())
    }
  }
  return seen.size > 0
}

export function buildTerminalSubAgentStubView(input: {
  trace: AgentTrace
  hostTool?: ToolCall | null
  scopedTraceMessages: readonly ChatMessage[]
  orphanTitle?: string
}): CollapsedSubAgentView {
  const persisted = input.trace.summaryLine?.trim()
  if (persisted) {
    return { summaryLine: persisted, liveLine: null }
  }
  const scoped = input.scopedTraceMessages
  const stats =
    scoped.length > 0
      ? computeSubAgentStatsFromMessages(scoped)
      : (input.trace.session?.stats ?? emptySubAgentToolStats())
  const orphan = input.hostTool ? '' : (input.orphanTitle?.trim() || '')
  return resolveCollapsedSubAgentView({
    orphanTitle: orphan,
    status: input.trace.status,
    stats,
    agentId: resolveTraceAgentId(input.trace),
    backgroundRunning: input.hostTool ? isBackgroundSubagentCall(input.hostTool) : false
  })
}

export function collectScopedToolCallIds(messages: readonly ChatMessage[]): string[] {
  const ids: string[] = []
  const seen = new Set<string>()
  for (const msg of messages) {
    for (const tc of msg.toolCalls ?? []) {
      const id = tc.id?.trim()
      if (!id || seen.has(id)) continue
      seen.add(id)
      ids.push(id)
    }
  }
  return ids
}

export function rememberTraceSearchToolCallIds(
  trace: AgentTrace,
  messages: readonly ChatMessage[]
): void {
  mergeSearchToolCallIds(trace, collectScopedToolCallIds(messages))
}

function mergeSearchToolCallIds(trace: AgentTrace, incoming: readonly string[]): void {
  if (!incoming.length) return
  const seen = new Set((trace.searchToolCallIds ?? []).map(id => id.trim()).filter(Boolean))
  const next = [...seen]
  for (const id of incoming) {
    const trimmed = id.trim()
    if (!trimmed || seen.has(trimmed)) continue
    seen.add(trimmed)
    next.push(trimmed)
  }
  trace.searchToolCallIds = next
}

function computeTraceSummaryLine(
  trace: AgentTrace,
  input: {
    hostTool?: ToolCall | null
    scopedTraceMessages: readonly ChatMessage[]
    orphanTitle?: string
  }
): string {
  const scoped = input.scopedTraceMessages
  const stats =
    scoped.length > 0
      ? computeSubAgentStatsFromMessages(scoped)
      : (trace.session?.stats ?? emptySubAgentToolStats())
  const orphan = input.hostTool ? '' : (input.orphanTitle?.trim() || '')
  return resolveCollapsedSubAgentView({
    orphanTitle: orphan,
    status: trace.status,
    stats,
    agentId: resolveTraceAgentId(trace),
    backgroundRunning: input.hostTool ? isBackgroundSubagentCall(input.hostTool) : false
  }).summaryLine.trim()
}

/** Persist collapsed summary on the trace for stub UI after scoped rows are omitted from hydrate. */
export function persistTerminalTraceSummaryLine(
  trace: AgentTrace,
  input: {
    hostTool?: ToolCall | null
    scopedTraceMessages: readonly ChatMessage[]
    orphanTitle?: string
  }
): void {
  // Always recompute — do not keep a stale line written before scoped rows arrived.
  const line = computeTraceSummaryLine(trace, input)
  if (line) trace.summaryLine = line
  rememberTraceSearchToolCallIds(trace, input.scopedTraceMessages)
}
