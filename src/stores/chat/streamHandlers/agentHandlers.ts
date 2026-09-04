import { ensureSubTrace, finalizeSubSession, isSubAgentTraceTerminal } from '../../../lib/subAgentSession'
import { persistTerminalTraceSummaryLine } from '../../../lib/subAgentFrameMount'
import { useConversationScopedStore } from '../../../lib/conversationScoped'
import type { StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type AgentStep = Extract<StreamEvent, { kind: 'agent_step' }>

export function handleAgentStep(ctx: StreamHandlerContext, e: AgentStep) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const depth = e.agent.depth ?? 0
  const terminal = isSubAgentTraceTerminal(e.agent.status)
  // Never revive a finished / cancelled lead row when a late or terminal agent_step arrives.
  const canMarkStreaming =
    !terminal &&
    r.msg.status !== 'cancelled' &&
    r.msg.status !== 'error' &&
    r.msg.status !== 'done'
  if (depth > 0) {
    const trace = ensureSubTrace(r.msg, e.agent.id, e.agent)
    trace.content = undefined
    if (canMarkStreaming && r.msg.status !== 'streaming') r.msg.status = 'streaming'
    if (terminal) {
      finalizeSubSession(trace)
      persistTerminalTraceSummaryLine(trace, {
        scopedTraceMessages: useConversationScopedStore().getRows(r.conv.id, {
          anchorMessageId: r.msg.id,
          traceId: trace.id,
          agentInstanceId: trace.agentInstanceId
        }),
        orphanTitle: (trace.parentToolCallId ?? '').trim() ? '' : trace.name
      })
      r.conv.updatedAt = Date.now()
    }
  } else {
    if (canMarkStreaming) r.msg.status = 'streaming'
    r.msg.agentId = e.agent.id
    r.msg.agentName = e.agent.name
    r.msg.agentTrace = r.msg.agentTrace || []
    const existing = r.msg.agentTrace.find(a => a.id === e.agent.id)
    if (existing) {
      const previousContent = existing.content || ''
      Object.assign(existing, e.agent)
      if (e.agent.content === undefined) existing.content = previousContent
    } else {
      r.msg.agentTrace.push(e.agent)
    }
  }
}
