import { agentInstanceIdFromTraceId, subAgentIdFromTraceId, subTaskIdFromTraceId } from '../subAgentStats'
import type { SpawnId, SpawnLookup } from './types'

/**
 * Resolve the store key for one spawn.
 * Prefer `agentInstanceId`; then instance embedded in explore/self trace ids;
 * last, a stable legacy bucket `anchor\\0traceId`.
 */
/** SpawnId / UUID lookups have no `task:agent` colon. Do not wrap those in the legacy composite key. */
export function looksLikeSpawnId(id: string): boolean {
  const trimmed = id.trim()
  return !!trimmed && !trimmed.includes(':')
}

export function resolveSpawnId(input: SpawnLookup): SpawnId {
  const instance = input.agentInstanceId?.trim()
  if (instance) return instance
  const fromTrace = agentInstanceIdFromTraceId(input.traceId ?? '')
  if (fromTrace) return fromTrace
  const anchor = input.anchorMessageId?.trim() ?? ''
  const trace = input.traceId?.trim() ?? ''
  if (looksLikeSpawnId(trace)) return trace
  if (anchor && trace) return `${anchor}\0${trace}`
  return trace
}

export function taskIdFromLookup(input: SpawnLookup): string {
  const explicit = input.taskId?.trim()
  if (explicit) return explicit
  const trace = input.traceId?.trim()
  return trace ? subTaskIdFromTraceId(trace) : ''
}

export function agentIdFromLookup(input: SpawnLookup): string {
  const explicit = input.agentId?.trim()
  if (explicit) return explicit
  const trace = input.traceId?.trim()
  return trace ? subAgentIdFromTraceId(trace) : ''
}
