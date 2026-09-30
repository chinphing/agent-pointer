import type { ChatMessage, ToolCall } from '../types/chat'

export function toolCallBaseName(name: string): string {
  const i = name.indexOf(':')
  return i === -1 ? name : name.slice(0, i)
}

/** Approval still needs its own surface on a collapsed turn. */
export function isPendingApprovalToolCall(tc: ToolCall): boolean {
  return tc.status === 'pending_approval'
}

/**
 * Pending `ask_user` — surfaced by the top-of-chat banner (design doc §4), never
 * by keeping a frame / collapsed row alive.
 */
export function isPendingAskUserToolCall(tc: ToolCall): boolean {
  const base = toolCallBaseName(tc.name)
  return base === 'ask_user'
    && (tc.status === 'pending' || tc.status === 'running')
}

/** In-flight `run_subagent` host — keep while collapsed so nested ask_user can surface. */
export function isInFlightSubagentHostToolCall(tc: ToolCall): boolean {
  if (toolCallBaseName(tc.name) !== 'run_subagent') return false
  return tc.status === 'pending' || tc.status === 'running'
}

/**
 * Collapsed-turn surface: direct approval tools, plus subagent hosts whose
 * child frame still needs a surface. A host can already be `success` while the
 * child is still running.
 * `Array.filter` / `some` pass the index as the second argument — ignore that.
 */
type CollapsedHost = {
  agentTrace?: ReadonlyArray<{
    status?: string
    parentToolCallId?: string
    session?: { toolCalls?: ToolCall[] }
  }>
}

export function isCollapsedSurfaceToolCall(
  tc: ToolCall,
  host?: CollapsedHost | number
): boolean {
  if (isPendingApprovalToolCall(tc) || isInFlightSubagentHostToolCall(tc)) return true
  if (!host || typeof host !== 'object') return false
  if (toolCallBaseName(tc.name) !== 'run_subagent') return false
  const id = tc.id.trim()
  if (!id) return false
  const linked = (host.agentTrace ?? []).filter(
    trace => (trace.parentToolCallId ?? '').trim() === id
  )
  return agentTraceNeedsCollapsedSurface(linked)
}

/**
 * Collapsed parent turn: mount sub-agent frames when the lead message or a
 * trailing tool group still has a child that needs a surface. Pass each
 * group's own message — a finished `run_subagent` is invisible without it.
 */
export function hostNeedsCollapsedSubAgentFrames(
  message: CollapsedHost & { toolCalls?: readonly ToolCall[] },
  trailingGroups?: readonly {
    toolCalls: readonly ToolCall[]
    message: CollapsedHost
  }[]
): boolean {
  if (agentTraceNeedsCollapsedSurface(message.agentTrace)) return true
  if ((message.toolCalls ?? []).some(tc => isCollapsedSurfaceToolCall(tc, message))) return true
  return (trailingGroups ?? []).some(group =>
    agentTraceNeedsCollapsedSurface(group.message.agentTrace)
    || group.toolCalls.some(tc => isCollapsedSurfaceToolCall(tc, group.message))
  )
}

/** True when a lead message's agentTrace still needs a collapsed-frame surface. */
export function agentTraceNeedsCollapsedSurface(
  traces: readonly { status?: string; session?: { toolCalls?: ToolCall[] } }[] | undefined
): boolean {
  // Running children keep their live frame. Pending `ask_user` no longer does —
  // the top-of-chat banner owns that entry point (design doc §4).
  return (traces ?? []).some(trace => trace.status === 'running')
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
  showSidecarCalls: boolean = false,
  showNonSidecarCalls: boolean = true
): ToolCall[] {
  const hidden = new Set(hideToolNames ?? [])
  return (
    toolCalls?.filter(tc => {
      const base = toolCallBaseName(tc.name)
      if (base === 'response') return false
      const isSidecar = isDefaultHiddenSidecarCall(tc.name, base)
      if (!showSidecarCalls && isSidecar) return false
      if (!showNonSidecarCalls && !isSidecar) return false
      // Debug override: when sidecar cards are explicitly enabled, do not let
      // manifest/default hideToolNames suppress sidecar tool calls.
      if (!(isSidecar && showSidecarCalls) && (hidden.has(tc.name) || hidden.has(base))) {
        return false
      }
      return true
    }) ?? []
  )
}

function isDefaultHiddenSidecarCall(fullName: string, baseName: string): boolean {
  return isSidecarToolCall(fullName, baseName)
}

/** Sidecar / host-only tools (verify, task_board) — hidden from default UI cards. */
export function isSidecarToolCall(fullName: string, baseName?: string): boolean {
  const name = fullName.trim()
  const base = (baseName ?? toolCallBaseName(name)).trim()
  if (
    name.startsWith('verify:') ||
    name === 'verify_report' ||
    base === 'verify' ||
    base.startsWith('verify_')
  ) {
    return true
  }
  return name.startsWith('task_board') || base.startsWith('task_board')
}

export function taskBoardPatchSummaryFromArgs(argumentsJson: string | undefined): string | null {
  if (!argumentsJson?.trim()) return null
  try {
    const args = JSON.parse(argumentsJson) as Record<string, unknown>
    const flatId = typeof args.item_id === 'string' ? args.item_id.trim() : typeof args.id === 'string' ? args.id.trim() : ''
    const flatStatus = typeof args.status === 'string' ? args.status.trim() : ''
    if (flatId && flatStatus) return `#${flatId} → ${flatStatus}`
    if (flatId) return `#${flatId}`
    const rawItems = args.items
    let items: unknown[] | null = null
    if (Array.isArray(rawItems)) items = rawItems
    else if (typeof rawItems === 'string') {
      try {
        const parsed = JSON.parse(rawItems)
        if (Array.isArray(parsed)) items = parsed
      } catch { /* ignore */ }
    }
    if (!items?.length) return null
    if (items.length === 1) {
      const row = items[0] as Record<string, unknown>
      const id = String(row.id ?? row.item_id ?? '?').trim()
      const status = typeof row.status === 'string' ? row.status.trim() : ''
      return status ? `#${id} → ${status}` : `#${id}`
    }
    return `更新 · ${items.length} 行`
  } catch {
    return null
  }
}

export function taskBoardToolSummary(result: string | undefined): string | null {
  if (!result?.trim()) return null
  try {
    const parsed = JSON.parse(result) as {
      method?: string
      board_len?: number
      patched?: Array<{ id?: string; status?: string }>
      summary?: { method?: string; count?: number }
      document?: { board?: unknown[]; global_milestones?: unknown[] }
    }
    const patched = parsed.patched
    if (Array.isArray(patched) && patched.length === 1) {
      const row = patched[0]
      const id = row?.id ?? '?'
      const status = row?.status
      if (status) return `#${id} → ${status}`
      return `#${id}`
    }
    if (Array.isArray(patched) && patched.length > 1) {
      return `patch · ${patched.length} 行`
    }
    const boardLen =
      parsed.board_len
      ?? parsed.document?.global_milestones?.length
      ?? parsed.document?.board?.length
      ?? parsed.summary?.count
    if (typeof boardLen === 'number') return `共 ${boardLen} 里程碑`
    // 只有 JSON 里确实出现任务板相关字段才生成摘要；否则（如 ask_user 的
    // {"selected": ...}）返回 null，避免把任意工具 result 误显示成"任务板 · …"。
    const hasBoardShape =
      typeof parsed.method === 'string'
      || typeof parsed.summary?.method === 'string'
    if (!hasBoardShape) return null
    const method = parsed.method ?? parsed.summary?.method ?? 'update'
    return `任务板 · ${method}`
  } catch {
    return '任务板已更新'
  }
}
