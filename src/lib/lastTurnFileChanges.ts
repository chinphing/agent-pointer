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
  // Sub-agent host stubs / nested user rows must not become the turn anchor —
  // baselines and Review use the real lead user message id.
  return message.role === 'user' && Boolean(message.id?.trim()) && !message.anchorMessageId?.trim()
}

/**
 * Latest user-anchored turn's successful file_edit / file_write paths.
 * Includes nested/scoped assistant tool calls that belong to the same lead turn.
 */
export function lastTurnFileChanges(messages: ChatMessage[] | undefined | null): LastTurnFileChanges | null {
  const list = messages ?? []
  let turnId: string | null = null
  let turnStart = -1
  for (let i = 0; i < list.length; i++) {
    const message = list[i]!
    if (isLeadUserMessage(message)) {
      turnId = message.id
      turnStart = i
    }
  }
  if (!turnId || turnStart < 0) return null

  const toolCalls = list
    .slice(turnStart)
    .flatMap(message => message.toolCalls ?? [])
    .filter(tc => tc.status === 'success' && isFileMutatingTool(tc))

  if (!toolCalls.length) return null

  const fromDiffs = buildFileChangeSummaries(toolCalls)
  const byPath = new Map<string, FileChangeSummary>()
  for (const summary of fromDiffs) {
    byPath.set(summary.path.replace(/\\/g, '/').toLocaleLowerCase(), summary)
  }

  for (const tc of toolCalls) {
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

  const files = [...byPath.values()].sort((a, b) => {
    const byName = a.fileName.localeCompare(b.fileName, undefined, { sensitivity: 'base' })
    if (byName !== 0) return byName
    return a.path.localeCompare(b.path, undefined, { sensitivity: 'base' })
  })
  return files.length ? { turnId, files } : null
}
