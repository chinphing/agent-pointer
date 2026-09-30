import type { AgentTrace, ChatMessage } from '../types/chat'

/**
 * Conversation-search targets that reach sub-agent frames.
 *
 * The search scans the lead transcript **and** the scoped rows (deep sub-agent
 * rounds), so a hit inside a frame is addressed by the ids the frame already
 * renders: `ToolCall.id` on a tool row, `ChatMessage.id` on a content block.
 */
export interface SubAgentSearchTargets {
  /** `ToolCall.id`s that matched the current query. */
  toolCallIds?: readonly string[]
  /** Scoped row ids whose `content` matched the current query. */
  contentMessageIds?: readonly string[]
}

function trimmedIdSet(ids?: readonly string[]): Set<string> {
  const set = new Set<string>()
  for (const raw of ids ?? []) {
    const id = raw?.trim()
    if (id) set.add(id)
  }
  return set
}

/** `true` when there is anything to look for (search closed → false, no row scans). */
export function hasSubAgentSearchTargets(targets: SubAgentSearchTargets): boolean {
  return trimmedIdSet(targets.toolCallIds).size > 0
    || trimmedIdSet(targets.contentMessageIds).size > 0
}

/** Does any of these rows carry a searched tool call or a searched content row? */
export function scopedRowsContainSearchTarget(
  rows: readonly ChatMessage[],
  targets: SubAgentSearchTargets
): boolean {
  const toolIds = trimmedIdSet(targets.toolCallIds)
  const contentIds = trimmedIdSet(targets.contentMessageIds)
  if (toolIds.size === 0 && contentIds.size === 0) return false
  for (const row of rows) {
    if (contentIds.has(row.id)) return true
    for (const tc of row.toolCalls ?? []) {
      if (toolIds.has(tc.id)) return true
    }
  }
  return false
}

/**
 * Traces spawned **by** `traceId` that are recorded on these rows.
 *
 * A frame only ever holds its own scoped rows, so its own trace is not part of the
 * set — the tree builder would drop the parent link and return every nested spawn as
 * a root. Nested spawns are therefore read straight off the rows' `agentTrace`, with
 * the same fallback the tree builder uses for legacy rows (owning instance = parent).
 */
function nestedTracesOnRows(rows: readonly ChatMessage[], traceId: string): AgentTrace[] {
  const parent = traceId.trim()
  if (!parent) return []
  const out: AgentTrace[] = []
  const seen = new Set<string>()
  for (const row of rows) {
    const owner = (row.agentInstanceId ?? '').trim() || (row.traceId ?? '').trim()
    for (const trace of row.agentTrace ?? []) {
      const id = (trace.id ?? '').trim()
      if (!id || id === parent || seen.has(id)) continue
      const explicit = (trace.parentTraceId ?? '').trim()
      const isChild = explicit ? explicit === parent : owner === parent
      if (!isChild) continue
      seen.add(id)
      out.push(trace)
    }
  }
  return out
}

/**
 * `true` when this trace — or any nested trace below it — owns a search hit.
 *
 * Frames render lazily (a collapsed parent does not mount its children), so a hit two
 * levels down has to expand its ancestors. `ownRows` are the rows of `trace` itself;
 * deeper levels are resolved through `rowsForTrace` (one scoped-store lookup per nested
 * spawn, only while a search is active).
 */
export function traceSubtreeContainsSearchTarget(input: {
  trace: AgentTrace
  ownRows: readonly ChatMessage[]
  targets: SubAgentSearchTargets
  rowsForTrace: (trace: AgentTrace) => readonly ChatMessage[]
}): boolean {
  if (!hasSubAgentSearchTargets(input.targets)) return false

  const visited = new Set<string>()
  const walk = (traceId: string, rows: readonly ChatMessage[]): boolean => {
    if (scopedRowsContainSearchTarget(rows, input.targets)) return true
    for (const child of nestedTracesOnRows(rows, traceId)) {
      const childId = (child.id ?? '').trim()
      if (!childId || visited.has(childId)) continue
      visited.add(childId)
      if (walk(childId, input.rowsForTrace(child))) return true
    }
    return false
  }

  const rootId = (input.trace.id ?? '').trim()
  if (rootId) visited.add(rootId)
  return walk(rootId, input.ownRows)
}
