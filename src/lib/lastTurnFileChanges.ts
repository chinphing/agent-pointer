import type { ChatMessage, ToolCall } from '../types/chat'
import { buildFileChangeSummaries, type FileChangeSummary } from './toolCallDisplay'
import { toolCallBaseName } from './messageTooling'
import { isScopedSubMessage } from './subAgentMessages'
import { isRealUserTaskMessage } from './threadLayoutGlue'

export type LastTurnFileChanges = {
  turnId: string
  files: FileChangeSummary[]
}

function isFileMutatingTool(tc: ToolCall): boolean {
  const base = toolCallBaseName(tc.name)
  return base === 'file_edit' || base === 'file_write'
}

function pathFromUnknownRecord(row: Record<string, unknown>): string {
  const path = row.path ?? row.file ?? row.file_path ?? row.filePath
  return typeof path === 'string' && path.trim() ? path.trim() : ''
}

function pathFromToolCall(tc: ToolCall): string {
  if (tc.result) {
    if (typeof tc.result === 'object' && tc.result && !Array.isArray(tc.result)) {
      const path = pathFromUnknownRecord(tc.result as Record<string, unknown>)
      if (path) return path
    } else {
      try {
        const parsed = JSON.parse(String(tc.result)) as unknown
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
          const path = pathFromUnknownRecord(parsed as Record<string, unknown>)
          if (path) return path
        }
      } catch {
        /* fall through */
      }
    }
  }
  try {
    const args = JSON.parse(tc.arguments || '{}') as Record<string, unknown>
    const path = pathFromUnknownRecord(args)
    if (path) return path
  } catch {
    /* ignore */
  }
  return ''
}

function pathBasename(path: string): string {
  const normalized = path.replace(/\\/g, '/')
  const parts = normalized.split('/')
  return parts[parts.length - 1] || path
}

export function isLeadUserMessage(message: ChatMessage): boolean {
  return isRealUserTaskMessage(message) && !isScopedSubMessage(message)
}

type LeadStartsMemo = {
  length: number
  starts: Array<{ turnId: string; start: number }>
}

const leadStartsMemo = new WeakMap<ChatMessage[], LeadStartsMemo>()

function scanLeadStarts(
  list: ChatMessage[],
  from: number,
  into: Array<{ turnId: string; start: number }>
): void {
  for (let i = from; i < list.length; i++) {
    const message = list[i]!
    if (isLeadUserMessage(message)) {
      into.push({ turnId: message.id, start: i })
    }
  }
}

/** Lead-user starts. Same array + same length returns the previous list (no rescan). */
export function collectLeadTurnStarts(
  list: ChatMessage[]
): Array<{ turnId: string; start: number }> {
  const prev = leadStartsMemo.get(list)
  if (prev && prev.length === list.length) return prev.starts
  if (prev && list.length > prev.length) {
    const added: Array<{ turnId: string; start: number }> = []
    scanLeadStarts(list, prev.length, added)
    if (added.length === 0) {
      leadStartsMemo.set(list, { length: list.length, starts: prev.starts })
      return prev.starts
    }
    const starts = prev.starts.concat(added)
    leadStartsMemo.set(list, { length: list.length, starts })
    return starts
  }
  const starts: Array<{ turnId: string; start: number }> = []
  scanLeadStarts(list, 0, starts)
  leadStartsMemo.set(list, { length: list.length, starts })
  return starts
}

export type ActiveTurnFileCache = {
  turnId: string
  settleKey: string
  files: FileChangeSummary[]
  mergedToolIds: Set<string>
}

function mutatingToolIds(toolCalls: ToolCall[]): Set<string> {
  return new Set(toolCalls.map(tc => tc.id))
}

function pathKey(path: string): string {
  return path.replace(/\\/g, '/').toLocaleLowerCase()
}

function sortFileChangeSummaries(files: FileChangeSummary[]): FileChangeSummary[] {
  return [...files].sort((a, b) => {
    const byName = a.fileName.localeCompare(b.fileName, undefined, { sensitivity: 'base' })
    if (byName !== 0) return byName
    return a.path.localeCompare(b.path, undefined, { sensitivity: 'base' })
  })
}

function mergeFileChangeSummaries(
  previous: FileChangeSummary[],
  incoming: FileChangeSummary[]
): FileChangeSummary[] {
  if (!incoming.length) return previous
  if (!previous.length) return incoming
  const byPath = new Map<string, FileChangeSummary>()
  for (const file of previous) {
    byPath.set(pathKey(file.path), file)
  }
  for (const file of incoming) {
    const key = pathKey(file.path)
    const existing = byPath.get(key)
    if (!existing) {
      byPath.set(key, file)
      continue
    }
    byPath.set(key, {
      ...existing,
      adds: existing.adds + file.adds,
      dels: existing.dels + file.dels,
      diffs: existing.diffs.concat(file.diffs),
      kind: file.kind === 'write' ? 'write' : existing.kind
    })
  }
  return sortFileChangeSummaries([...byPath.values()])
}

function keepPreviousSettle(
  previous: ActiveTurnFileCache,
  settleKey: string
): ActiveTurnFileCache {
  previous.settleKey = settleKey
  return previous
}

function mutatingToolsKey(toolCalls: ToolCall[]): string {
  return toolCalls.map(tc => tc.id).join('\0')
}

/**
 * Active turn: merge each successful file_edit / file_write as soon as it
 * completes. Do not wait for later search/read tools on the same assistant
 * message — those stay inflight for the whole ReAct loop.
 */
export function resolveActiveTurnFileChanges(
  list: ChatMessage[],
  turnId: string,
  start: number,
  previous: ActiveTurnFileCache | null,
  extra: readonly ChatMessage[] = []
): ActiveTurnFileCache {
  const mutating = collectSuccessfulFileMutations(list, start, list.length, extra)
  const settleKey = mutatingToolsKey(mutating)
  if (previous && previous.turnId === turnId && previous.settleKey === settleKey) {
    return previous
  }
  if (previous && previous.turnId === turnId) {
    const incomingTools = mutating.filter(tc => !previous.mergedToolIds.has(tc.id))
    if (!incomingTools.length) {
      return keepPreviousSettle(previous, settleKey)
    }
    const mergedToolIds = new Set(previous.mergedToolIds)
    for (const tc of incomingTools) mergedToolIds.add(tc.id)
    return {
      turnId,
      settleKey,
      files: mergeFileChangeSummaries(previous.files, filesFromToolCalls(incomingTools)),
      mergedToolIds
    }
  }
  return {
    turnId,
    settleKey,
    files: filesFromToolCalls(mutating),
    mergedToolIds: mutatingToolIds(mutating)
  }
}

function pushSuccessfulFileMutations(
  into: ToolCall[],
  seen: Set<string>,
  calls: ToolCall[] | undefined
): void {
  if (!calls?.length) return
  for (const tc of calls) {
    if (!isFileMutatingTool(tc)) continue
    if (tc.status !== 'success') continue
    if (seen.has(tc.id)) continue
    seen.add(tc.id)
    into.push(tc)
  }
}

function pushSuccessfulFileMutationsFromMessage(into: ToolCall[], seen: Set<string>, message: ChatMessage | undefined): void {
  if (!message) return
  pushSuccessfulFileMutations(into, seen, message.toolCalls)
  for (const trace of message.agentTrace ?? []) {
    pushSuccessfulFileMutations(into, seen, trace.session?.toolCalls)
  }
}

function collectSuccessfulFileMutations(
  list: ChatMessage[],
  start: number,
  end: number,
  extra: readonly ChatMessage[] = []
): ToolCall[] {
  const mutating: ToolCall[] = []
  const seen = new Set<string>()
  for (let i = start; i < end; i++) {
    pushSuccessfulFileMutationsFromMessage(mutating, seen, list[i])
  }
  for (const message of extra) {
    pushSuccessfulFileMutationsFromMessage(mutating, seen, message)
  }
  return mutating
}

export function fileChangesInRange(
  list: ChatMessage[],
  start: number,
  end: number,
  extra: readonly ChatMessage[] = []
): FileChangeSummary[] {
  return filesFromToolCalls(collectSuccessfulFileMutations(list, start, end, extra))
}

function filesFromToolCalls(toolCalls: ToolCall[]): FileChangeSummary[] {
  const mutating = toolCalls
  if (!mutating.length) return []

  const fromDiffs = buildFileChangeSummaries(mutating)
  const byPath = new Map<string, FileChangeSummary>()
  for (const summary of fromDiffs) {
    byPath.set(pathKey(summary.path), summary)
  }

  for (const tc of mutating) {
    const path = pathFromToolCall(tc)
    if (!path) continue
    const key = pathKey(path)
    if (byPath.has(key)) continue
    byPath.set(key, {
      path,
      fileName: pathBasename(path),
      kind: toolCallBaseName(tc.name) === 'file_write' ? 'write' : 'edit',
      adds: 0,
      dels: 0,
      diffs: []
    })
  }

  const files = sortFileChangeSummaries([...byPath.values()])
  if (!files.length) {
    console.warn('[turnFileChanges] successful file tools produced no path summaries', {
      count: mutating.length,
      tools: mutating.map(tc => ({
        id: tc.id,
        name: tc.name,
        status: tc.status,
        hasResult: Boolean(tc.result),
        hasArguments: Boolean(tc.arguments?.trim())
      }))
    })
  }
  return files
}

export type FrozenFileChangesCache = {
  key: string
  map: Map<string, FileChangeSummary[]>
}

export function closedLeadTurnsKey(
  starts: Array<{ turnId: string; start: number }>
): string {
  if (starts.length < 2) return ''
  return starts
    .slice(0, -1)
    .map(item => `${item.turnId}@${item.start}`)
    .join('\0')
}

/**
 * Closed-turn file lists. Reuses previous arrays when only a new lead turn appears;
 * rebuilds when history is prepended or the window is replaced.
 */
export function frozenFileChangesFromStarts(
  list: ChatMessage[],
  starts: Array<{ turnId: string; start: number }>,
  previous: FrozenFileChangesCache | null,
  extra: readonly ChatMessage[] = []
): FrozenFileChangesCache {
  const key = closedLeadTurnsKey(starts)
  if (previous && previous.key === key) return previous
  if (starts.length < 2) return { key: '', map: new Map() }

  const previousIds = previous?.key ? previous.key.split('\0') : []
  const nextIds = key.split('\0')
  const canAppend = Boolean(
    previous
    && nextIds.length === previousIds.length + 1
    && previousIds.every((id, index) => id === nextIds[index])
  )
  if (canAppend && previous) {
    const newlyClosed = starts[starts.length - 2]!
    const end = starts[starts.length - 1]!.start
    const files = fileChangesInRange(list, newlyClosed.start, end, extra)
    const map = new Map(previous.map)
    if (files.length) map.set(newlyClosed.turnId, files)
    else map.delete(newlyClosed.turnId)
    return { key, map }
  }

  const map = new Map<string, FileChangeSummary[]>()
  for (let i = 0; i < starts.length - 1; i++) {
    const range = starts[i]!
    const files = fileChangesInRange(list, range.start, starts[i + 1]!.start, extra)
    if (files.length) map.set(range.turnId, files)
  }
  return { key, map }
}

export function fileChangesByTurn(
  messages: ChatMessage[] | undefined | null
): Map<string, FileChangeSummary[]> {
  const list = messages ?? []
  const starts = collectLeadTurnStarts(list)
  const out = new Map<string, FileChangeSummary[]>()
  for (let i = 0; i < starts.length; i++) {
    const range = starts[i]!
    const files = fileChangesInRange(list, range.start, starts[i + 1]?.start ?? list.length)
    if (files.length) out.set(range.turnId, files)
  }
  return out
}

export function fileChangesForTurn(
  messages: ChatMessage[] | undefined | null,
  turnId: string
): LastTurnFileChanges | null {
  const list = messages ?? []
  const starts = collectLeadTurnStarts(list)
  const index = starts.findIndex(item => item.turnId === turnId)
  if (index < 0) return null
  const files = fileChangesInRange(list, starts[index]!.start, starts[index + 1]?.start ?? list.length)
  return files.length ? { turnId, files } : null
}

/**
 * Latest user-anchored turn's successful file_edit / file_write paths.
 * Includes nested/scoped assistant tool calls and agentTrace.session writes
 * that belong to the same lead turn.
 * Only reads the last lead-turn slice (not earlier turns' tools).
 */
export function lastTurnFileChanges(messages: ChatMessage[] | undefined | null): LastTurnFileChanges | null {
  const list = messages ?? []
  const starts = collectLeadTurnStarts(list)
  const last = starts[starts.length - 1]
  if (!last) return null
  const files = fileChangesInRange(list, last.start, list.length)
  return files.length ? { turnId: last.turnId, files } : null
}
