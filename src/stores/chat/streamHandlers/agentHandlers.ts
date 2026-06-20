import { ensureSubTrace, finalizeSubSession } from '../../../lib/subAgentSession'
import type { StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type AgentStep = Extract<StreamEvent, { kind: 'agent_step' }>
type SupervisorPlan = Extract<StreamEvent, { kind: 'supervisor_plan' }>

export function handleAgentStep(ctx: StreamHandlerContext, e: AgentStep) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const depth = e.agent.depth ?? 0
  if (depth > 0) {
    const trace = ensureSubTrace(r.msg, e.agent.id, e.agent)
    trace.content = undefined
    r.msg.status = 'streaming'
    if (e.agent.status === 'completed' || e.agent.status === 'failed') {
      finalizeSubSession(trace)
    }
    r.conv.updatedAt = Date.now()
  } else {
    r.msg.status = 'streaming'
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

export function handleSupervisorPlan(ctx: StreamHandlerContext, e: SupervisorPlan) {
  const r = ctx.findMessage(e.messageId)
  if (!r || r.conv.id !== e.conversationId) return
  r.msg.status = 'streaming'
  r.msg.supervisorPlanTasks = e.tasks
  void ctx.refreshTaskBoard(e.conversationId)
}
