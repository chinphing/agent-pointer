import type { ChatMessage, ToolCall } from '../types/chat'
import { buildFileChangeSummaries, type FileChangeSummary } from './toolCallDisplay'
import { toolCallBaseName } from './messageTooling'

export type LastTurnFileChanges = {
  turnId: string
  files: FileChangeSummary[]
}

function isFileMutatingTool(tc: ToolCall): boolean {
  const base = toolCallBaseName(tc.name)
  return base === 'file_edit' || base === 'file_write'
}

function pathFromToolCall(tc: ToolCall): string {
  if (tc.result) {
    try {
      const parsed = JSON.parse(tc.result) as { path?: unknown; success?: unknown }
      if (parsed.success === true && typeof parsed.path === 'string' && parsed.path.trim()) {
        return parsed.path.trim()
      }
    } catch {
      /* fall through */
    }
  }
  try {
    const args = JSON.parse(tc.arguments || '{}') as Record<string, unknown>
    const path = args.path ?? args.file
    if (typeof path === 'string' && path.trim()) return path.trim()
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

function isLeadUserMessage(message: ChatMessage): boolean {
  return message.role === 'user' && Boolean(message.id?.trim()) && !message.anchorMessageId?.trim()
}

function filesFromToolCalls(toolCalls: ToolCall[]): FileChangeSummary[] {
  const mutating = toolCalls.filter(tc => tc.status === 'success' && isFileMutatingTool(tc))
  if (!mutating.length) return []

  const fromDiffs = buildFileChangeSummaries(mutating)
  const byPath = new Map<string, FileChangeSummary>()
  for (const summary of fromDiffs) {
    byPath.set(summary.path.replace(/\\/g, '/').toLocaleLowerCase(), summary)
  }

  for (const tc of mutating) {
    const path = pathFromToolCall(tc)
    if (!path) continue
    const key = path.replace(/\\/g, '/').toLocaleLowerCase()
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

  return [...byPath.values()].sort((a, b) => {
    const byName = a.fileName.localeCompare(b.fileName, undefined, { sensitivity: 'base' })
    if (byName !== 0) return byName
    return a.path.localeCompare(b.path, undefined, { sensitivity: 'base' })
  })
}

/** Lead-user turn ranges: [start, end) in transcript order. */
function leadTurnRanges(list: ChatMessage[]): Array<{ turnId: string; start: number; end: number }> {
  const starts: Array<{ turnId: string; start: number }> = []
  for (let i = 0; i < list.length; i++) {
    const message = list[i]!
    if (isLeadUserMessage(message)) {
      starts.push({ turnId: message.id, start: i })
    }
  }
  return starts.map((item, index) => ({
    turnId: item.turnId,
    start: item.start,
    end: starts[index + 1]?.start ?? list.length
  }))
}

export function fileChangesByTurn(
  messages: ChatMessage[] | undefined | null
): Map<string, FileChangeSummary[]> {
  const list = messages ?? []
  const out = new Map<string, FileChangeSummary[]>()
  for (const range of leadTurnRanges(list)) {
    const toolCalls = list
      .slice(range.start, range.end)
      .flatMap(message => message.toolCalls ?? [])
    const files = filesFromToolCalls(toolCalls)
    if (files.length) out.set(range.turnId, files)
  }
  return out
}

export function fileChangesForTurn(
  messages: ChatMessage[] | undefined | null,
  turnId: string
): LastTurnFileChanges | null {
  const files = fileChangesByTurn(messages).get(turnId)
  return files?.length ? { turnId, files } : null
}

/**
 * Latest user-anchored turn's successful file_edit / file_write paths.
 * Includes nested/scoped assistant tool calls that belong to the same lead turn.
 */
export function lastTurnFileChanges(messages: ChatMessage[] | undefined | null): LastTurnFileChanges | null {
  const list = messages ?? []
  let turnId: string | null = null
  for (const message of list) {
    if (isLeadUserMessage(message)) turnId = message.id
  }
  if (!turnId) return null
  return fileChangesForTurn(list, turnId)
}
