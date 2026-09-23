import type { ChatMessage, ToolCall } from '../types/chat'
import { effectiveToolDisplaySummary, fileToolDisplayPath, rememberFileChange } from './toolCallDisplay'

/** UTF-16 code units. One body string over this clears the whole group. */
export const TOOL_BODY_CHAR_LIMIT = 4096

export const TERMINAL_OUTPUT_TRUNCATED_PREFIX = '...[output truncated]\n'

const BODY_KEYS = ['arguments', 'result', 'terminalOutput', 'webSearchOutput'] as const

type BodyKey = (typeof BODY_KEYS)[number]

function bodyLength(tc: ToolCall, key: BodyKey): number {
  const value = tc[key]
  return typeof value === 'string' ? value.length : 0
}

function isActiveToolStatus(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

/**
 * Keep the tail of a live terminal stream.
 * `maxChars` is `terminalOutputMaxBytes` used as a code-unit cap.
 */
export function capTerminalOutput(
  existing: string,
  chunk: string,
  maxChars: number
): { text: string; newlyTruncated: boolean } {
  const limit = Math.max(1, Math.floor(maxChars))
  const prev = existing
  const next = prev + chunk
  if (next.length <= limit) return { text: next, newlyTruncated: false }
  const keep = Math.max(1, limit - TERMINAL_OUTPUT_TRUNCATED_PREFIX.length)
  return {
    text: TERMINAL_OUTPUT_TRUNCATED_PREFIX + next.slice(next.length - keep),
    newlyTruncated: !prev.startsWith(TERMINAL_OUTPUT_TRUNCATED_PREFIX)
  }
}

/**
 * Drop the body group when any string exceeds the limit.
 * Returns how many characters were cleared. Already-evicted calls stay evicted.
 * Active calls are left alone so argument and output deltas can still append.
 */
export function slimToolCallBody(tc: ToolCall): number {
  if (isActiveToolStatus(tc.status)) return 0
  if (tc.bodyEvicted) return 0
  rememberFileChange(tc)
  if (!tc.displaySummary?.trim()) {
    const summary = fileToolDisplayPath(tc) || effectiveToolDisplaySummary(tc)
    if (summary) tc.displaySummary = summary
  }
  const longest = Math.max(...BODY_KEYS.map(key => bodyLength(tc, key)))
  if (longest <= TOOL_BODY_CHAR_LIMIT) {
    tc.bodyEvicted = false
    return 0
  }
  let cleared = 0
  for (const key of BODY_KEYS) cleared += bodyLength(tc, key)
  tc.arguments = ''
  delete tc.result
  delete tc.terminalOutput
  delete tc.webSearchOutput
  tc.bodyEvicted = true
  return cleared
}

/** Copy the body group from a disk row onto the in-memory tool call. */
export function restoreToolCallBody(target: ToolCall, source: ToolCall): void {
  target.arguments = source.arguments ?? ''
  if (source.result !== undefined) target.result = source.result
  else delete target.result
  if (source.terminalOutput !== undefined) target.terminalOutput = source.terminalOutput
  else delete target.terminalOutput
  if (source.webSearchOutput !== undefined) target.webSearchOutput = source.webSearchOutput
  else delete target.webSearchOutput
  target.bodyEvicted = false
}

export function toolCallOnMessage(msg: ChatMessage, toolCallId: string): ToolCall | null {
  const id = toolCallId.trim()
  if (!id) return null
  for (const tc of msg.toolCalls ?? []) {
    if (tc.id === id) return tc
  }
  for (const trace of msg.agentTrace ?? []) {
    for (const tc of trace.session?.toolCalls ?? []) {
      if (tc.id === id) return tc
    }
  }
  return null
}

/**
 * Slim finished tool calls on one message.
 * `skipToolCallId` keeps the row the user currently has open.
 * Returns characters cleared.
 */
export function slimMessageForMemory(msg: ChatMessage, skipToolCallId?: string): number {
  let cleared = 0
  if (msg.role === 'tool' && msg.content) {
    cleared += msg.content.length
    msg.content = ''
  }
  const skip = skipToolCallId?.trim() ?? ''
  const visit = (tc: ToolCall) => {
    if (skip && tc.id === skip) return
    cleared += slimToolCallBody(tc)
  }
  for (const tc of msg.toolCalls ?? []) visit(tc)
  for (const trace of msg.agentTrace ?? []) {
    for (const tc of trace.session?.toolCalls ?? []) visit(tc)
  }
  return cleared
}

export function slimMessagesForMemory(messages: ChatMessage[], skipToolCallId?: string): ChatMessage[] {
  for (const msg of messages) slimMessageForMemory(msg, skipToolCallId)
  return messages
}
