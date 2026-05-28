import type { ChatMessage, ToolCall } from '../types/chat'

export function toolCallBaseName(name: string): string {
  const i = name.indexOf(':')
  return i === -1 ? name : name.slice(0, i)
}

/** Assistant row used the `response` tool (final user-visible reply), not an intermediate tool round. */
export function isResponseAssistantMessage(
  message: Pick<ChatMessage, 'toolCalls' | 'toolNamePreview' | 'responseTextDraft' | 'content'>
): boolean {
  const tcs = message.toolCalls ?? []
  if (tcs.some(tc => toolCallBaseName(tc.name) !== 'response')) return false
  if (message.responseTextDraft?.trim()) return true
  const preview = message.toolNamePreview?.trim()
  if (preview) return toolCallBaseName(preview) === 'response'
  return !!(message.content?.trim())
}

/** `response` tool is not shown as a card (matches backend). */
export function visibleToolCalls(
  toolCalls: ToolCall[] | undefined,
  hideToolNames?: string[],
  showSidecarCalls: boolean = false
): ToolCall[] {
  const hidden = new Set(hideToolNames ?? [])
  return (
    toolCalls?.filter(tc => {
      const base = toolCallBaseName(tc.name)
      if (base === 'response') return false
      if (!showSidecarCalls && isDefaultHiddenSidecarCall(tc.name, base)) return false
      if (hidden.has(tc.name) || hidden.has(base)) return false
      return true
    }) ?? []
  )
}

function isDefaultHiddenSidecarCall(fullName: string, baseName: string): boolean {
  return (
    fullName.startsWith('verify:') ||
    fullName.startsWith('task_board:') ||
    baseName === 'verify' ||
    baseName === 'task_board'
  )
}

export function taskBoardToolSummary(result: string | undefined): string | null {
  if (!result?.trim()) return null
  try {
    const parsed = JSON.parse(result) as { summary?: { method?: string; count?: number }; document?: { board?: unknown[] } }
    const method = parsed.summary?.method ?? 'update'
    const count = parsed.document?.board?.length ?? parsed.summary?.count
    if (typeof count === 'number') return `任务板 · ${method} · ${count} 项`
    return `任务板 · ${method}`
  } catch {
    return '任务板已更新'
  }
}
