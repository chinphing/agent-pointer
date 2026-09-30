import type { AgentTrace, ChatMessage } from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { subAgentIdFromTraceId } from './subAgentStats'
import { toolCallBaseName } from './messageTooling'

const COMPUTER_TOOL_BASES = new Set([
  'mouse',
  'keyboard',
  'hotkey',
  'clipboard',
  'screenshot',
  'scroll',
  'type',
  'wait',
  'input',
  'modified_click'
])

export function isComputerToolBase(base: string): boolean {
  const b = base.trim()
  if (!b) return false
  if (COMPUTER_TOOL_BASES.has(b)) return true
  if (b === 'launch_app' || b === 'list_apps') return true
  if (b.startsWith('mouse_')) return true
  if (b.startsWith('input_')) return true
  if (b.startsWith('modified_click_')) return true
  if (b.startsWith('clipboard_')) return true
  return false
}

export function isComputerToolName(name: string): boolean {
  return isComputerToolBase(toolCallBaseName(name))
}

/** Lead trace id is `computer`; delegated traces use `{taskId}:computer`. */
export function isComputerAgentTrace(
  trace: Pick<AgentTrace, 'id' | 'agentId'>
): boolean {
  const agentId = (trace.agentId?.trim() || subAgentIdFromTraceId(trace.id)).trim()
  return (agentId || trace.id.trim()) === 'computer'
}

/** Sub-task id for delegated computer only; lead trace `computer` returns null. */
export function delegatedComputerSubTaskId(
  trace: Pick<AgentTrace, 'id' | 'taskId'> | undefined
): string | null {
  const explicit = trace?.taskId?.trim()
  if (explicit) return explicit
  const id = trace?.id.trim()
  if (!id || !id.includes(':')) return null
  const taskId = id.slice(0, id.indexOf(':')).trim()
  return taskId || null
}

function toolInProgress(status: string): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

function messageHasInProgressComputerTool(message: ChatMessage): boolean {
  const onMessage = (message.toolCalls ?? []).some(
    tc => isComputerToolName(tc.name) && toolInProgress(tc.status)
  )
  if (onMessage) return true
  for (const trace of message.agentTrace ?? []) {
    if (!isComputerAgentTrace(trace)) continue
    const calls = trace.session?.toolCalls ?? []
    if (calls.some(tc => isComputerToolName(tc.name) && toolInProgress(tc.status))) {
      return true
    }
  }
  return false
}

/** True while computer agent (lead or sub-agent) is actively executing. */
export function isComputerExecuting(
  generating: boolean,
  leadAgentId: string | undefined,
  message: ChatMessage | undefined
): boolean {
  if (!generating) return false

  const lead = leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  if (lead === 'computer') return true

  if (!message) return false
  if (message.agentId === 'computer') return true
  if (message.agentTrace?.some(t => isComputerAgentTrace(t) && t.status === 'running')) return true
  if (messageHasInProgressComputerTool(message)) return true

  return false
}

export function activeComputerTrace(message: ChatMessage | undefined) {
  if (!message) return undefined
  return message.agentTrace?.find(t => isComputerAgentTrace(t) && t.status === 'running')
}

function isLeadComputerAgent(leadAgentId: string | undefined): boolean {
  const lead = leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  return lead === 'computer'
}

/**
 * Whether the desktop client should shrink to the compact dock bar.
 * Lead computer always shrinks; delegated computer shrinks only when the task
 * target is external apps (not Pointer itself).
 */
export function shouldShrinkComputerWindow(
  generating: boolean,
  leadAgentId: string | undefined,
  message: ChatMessage | undefined
): boolean {
  if (!isComputerExecuting(generating, leadAgentId, message)) return false
  if (isLeadComputerAgent(leadAgentId)) return true
  const trace = activeComputerTrace(message)
  if (trace?.computerTarget === 'self') return false
  return true
}
