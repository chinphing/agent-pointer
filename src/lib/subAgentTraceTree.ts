import type { AgentTrace, ChatMessage } from '../types/chat'

/**
 * Read-time view of the nested sub-agent trace tree.
 *
 * The persisted source of truth stays where it always was: a trace lives on **its own
 * layer's** row — depth 1 on the lead assistant message, deeper levels on the parent
 * worker's scoped row. This module only merges those layers and re-links them through
 * `AgentTrace.parentTraceId`, so the renderer can nest frames instead of losing every
 * level below the first one.
 */
export interface SubAgentTraceTree {
  /** Every merged trace, lead-level first, deduped by trace id. */
  all: AgentTrace[]
  /** Traces with no parent inside the merged set (rendered under their host tool row). */
  roots: AgentTrace[]
  /** Direct children of `traceId`, in merge order. */
  childrenOf(traceId: string): AgentTrace[]
  /** Parent of `traceId` when it resolves inside the merged set. */
  parentOf(traceId: string): AgentTrace | undefined
}

/** Suffix appended to a self fork's label so it cannot be mistaken for a real worker. */
export const SELF_FORK_LABEL_SUFFIX = ' (fork)'

/** `true` for a self fork (`agentId: "self"`), `false` for a registered worker. */
export function isSelfForkTrace(
  trace: { delegation?: string | null } | null | undefined
): boolean {
  return (trace?.delegation ?? '').trim().toLowerCase() === 'self'
}

/** `coder` → `coder (fork)` for self forks; registered workers keep their label. */
export function selfForkTraceLabel(
  base: string,
  trace: { delegation?: string | null } | null | undefined
): string {
  const label = (base ?? '').trim()
  if (!label || !isSelfForkTrace(trace)) return base ?? ''
  if (label.endsWith(SELF_FORK_LABEL_SUFFIX)) return label
  return `${label}${SELF_FORK_LABEL_SUFFIX}`
}

function traceKey(trace: AgentTrace | undefined | null): string {
  return (trace?.id ?? '').trim()
}

interface TraceEntry {
  trace: AgentTrace
  /** Instance id owning the scoped row this trace was found on ('' for lead-level). */
  ownerInstanceId: string
}

function collectEntries(input: {
  leadTraces?: readonly AgentTrace[] | null
  scopedRows?: readonly ChatMessage[] | null
}): TraceEntry[] {
  const seen = new Set<string>()
  const out: TraceEntry[] = []
  const push = (trace: AgentTrace | undefined, ownerInstanceId: string) => {
    const key = traceKey(trace)
    if (!trace || !key || seen.has(key)) return
    seen.add(key)
    out.push({ trace, ownerInstanceId })
  }
  for (const trace of input.leadTraces ?? []) push(trace, '')
  for (const row of input.scopedRows ?? []) {
    const owner =
      (row.agentInstanceId ?? '').trim()
      || (row.traceId ?? '').trim()
    for (const trace of row.agentTrace ?? []) push(trace, owner)
  }
  return out
}

/**
 * Merge the lead message's traces with every scoped row's traces and link them by
 * `parentTraceId`.
 *
 * Backward compatible: traces without `parentTraceId` fall back to the instance that
 * owns the row they were persisted on (legacy nested rows), and traces whose parent is
 * not in the merged set stay roots so nothing silently disappears.
 */
export function buildSubAgentTraceTree(input: {
  leadTraces?: readonly AgentTrace[] | null
  scopedRows?: readonly ChatMessage[] | null
}): SubAgentTraceTree {
  const entries = collectEntries(input)
  const byId = new Map<string, TraceEntry>()
  for (const entry of entries) byId.set(traceKey(entry.trace), entry)

  const parentById = new Map<string, string>()
  for (const entry of entries) {
    const id = traceKey(entry.trace)
    const explicit = (entry.trace.parentTraceId ?? '').trim()
    let parentId = explicit && explicit !== id ? explicit : ''
    if (!parentId && entry.ownerInstanceId && entry.ownerInstanceId !== id) {
      // Legacy / degraded rows: the owning worker's row is the parent layer.
      parentId = entry.ownerInstanceId
    }
    if (parentId && !byId.has(parentId)) parentId = ''
    if (parentId) parentById.set(id, parentId)
  }

  // Defensive: malformed linkage must not hang the renderer.
  for (const [id, parentId] of [...parentById]) {
    const walked = new Set<string>([id])
    let cursor: string | undefined = parentId
    while (cursor) {
      if (walked.has(cursor)) {
        parentById.delete(id)
        break
      }
      walked.add(cursor)
      cursor = parentById.get(cursor)
    }
  }

  const childrenById = new Map<string, AgentTrace[]>()
  const roots: AgentTrace[] = []
  for (const entry of entries) {
    const id = traceKey(entry.trace)
    const parentId = parentById.get(id)
    if (!parentId) {
      roots.push(entry.trace)
      continue
    }
    const siblings = childrenById.get(parentId)
    if (siblings) siblings.push(entry.trace)
    else childrenById.set(parentId, [entry.trace])
  }

  return {
    all: entries.map(entry => entry.trace),
    roots,
    childrenOf: (traceId: string) => childrenById.get(traceId.trim()) ?? [],
    parentOf: (traceId: string) => {
      const parentId = parentById.get(traceId.trim())
      return parentId ? byId.get(parentId)?.trace : undefined
    }
  }
}

/**
 * Traces that render directly under their host `run_subagent` row: no `parentTraceId`,
 * or a parent that is not part of this list (so the frame is not silently dropped).
 */
export function rootTracesOf(traces: readonly AgentTrace[] | null | undefined): AgentTrace[] {
  const list = traces ?? []
  const ids = new Set(list.map(trace => traceKey(trace)).filter(Boolean))
  return list.filter(trace => {
    const id = traceKey(trace)
    const parent = (trace.parentTraceId ?? '').trim()
    if (!parent || parent === id) return true
    return !ids.has(parent)
  })
}
