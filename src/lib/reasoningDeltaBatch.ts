/** Batch reactive UI writes for streamed text to reduce Vue re-render churn. */

export const REASONING_DELTA_BATCH_MS = 200
export const CONTENT_DELTA_BATCH_MS = 50
export const TOOL_ARGS_DELTA_BATCH_MS = 80
export const TOOL_OUTPUT_DELTA_BATCH_MS = 80
export const WEB_SEARCH_OUTPUT_DELTA_BATCH_MS = 80

export type StreamDeltaApply = (
  messageId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  text: string
) => void

export type ReasoningDeltaApply = StreamDeltaApply
export type ContentDeltaApply = StreamDeltaApply

/** Tool-call scoped delta apply (args / terminal output / web search output). */
export type ToolDeltaApply = (
  messageId: string,
  toolCallId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  text: string
) => void

export type ToolArgsDeltaApply = ToolDeltaApply
export type ToolOutputDeltaApply = ToolDeltaApply
export type WebSearchOutputDeltaApply = ToolDeltaApply

type DeltaBuffer = {
  pending: Map<string, string>
  timers: Map<string, ReturnType<typeof setTimeout>>
  getApplyHandler: () => StreamDeltaApply | null
  label: string
}

type ToolDeltaBuffer = {
  pending: Map<string, string>
  timers: Map<string, ReturnType<typeof setTimeout>>
  getApplyHandler: () => ToolDeltaApply | null
  label: string
}

const reasoningPending = new Map<string, string>()
const reasoningTimers = new Map<string, ReturnType<typeof setTimeout>>()
const contentPending = new Map<string, string>()
const contentTimers = new Map<string, ReturnType<typeof setTimeout>>()
const toolArgsPending = new Map<string, string>()
const toolArgsTimers = new Map<string, ReturnType<typeof setTimeout>>()
const toolOutputPending = new Map<string, string>()
const toolOutputTimers = new Map<string, ReturnType<typeof setTimeout>>()
const webSearchOutputPending = new Map<string, string>()
const webSearchOutputTimers = new Map<string, ReturnType<typeof setTimeout>>()

let reasoningApplyHandler: ReasoningDeltaApply | null = null
let contentApplyHandler: ContentDeltaApply | null = null
let toolArgsApplyHandler: ToolArgsDeltaApply | null = null
let toolOutputApplyHandler: ToolOutputDeltaApply | null = null
let webSearchOutputApplyHandler: WebSearchOutputDeltaApply | null = null

function bufferKey(messageId: string, traceId?: string, scopedMessageId?: string): string {
  const tid = traceId?.trim()
  const sid = scopedMessageId?.trim()
  if (sid) return `${messageId}\0${tid ?? ''}\0${sid}`
  if (tid) return `${messageId}\0${tid}`
  return messageId
}

function toolBufferKey(
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): string {
  const tid = traceId?.trim()
  const sid = scopedMessageId?.trim()
  if (sid) return `${messageId}\0${toolCallId}\0${tid ?? ''}\0${sid}`
  if (tid) return `${messageId}\0${toolCallId}\0${tid}`
  return `${messageId}\0${toolCallId}`
}

export function setReasoningDeltaApplyHandler(handler: ReasoningDeltaApply | null): void {
  reasoningApplyHandler = handler
}

export function setContentDeltaApplyHandler(handler: ContentDeltaApply | null): void {
  contentApplyHandler = handler
}

export function setToolArgsDeltaApplyHandler(handler: ToolArgsDeltaApply | null): void {
  toolArgsApplyHandler = handler
}

export function setToolOutputDeltaApplyHandler(handler: ToolOutputDeltaApply | null): void {
  toolOutputApplyHandler = handler
}

export function setWebSearchOutputDeltaApplyHandler(handler: WebSearchOutputDeltaApply | null): void {
  webSearchOutputApplyHandler = handler
}

function enqueueDelta(
  buffer: DeltaBuffer,
  messageId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  const key = bufferKey(messageId, traceId, scopedMessageId)
  buffer.pending.set(key, (buffer.pending.get(key) ?? '') + text)
  if (buffer.timers.has(key)) return
  buffer.timers.set(key, setTimeout(() => flushDeltaKey(buffer, key), batchMs))
}

function flushDeltaKey(buffer: DeltaBuffer, key: string): void {
  const timer = buffer.timers.get(key)
  if (timer != null) {
    clearTimeout(timer)
    buffer.timers.delete(key)
  }
  const batch = buffer.pending.get(key)
  if (!batch) return
  buffer.pending.delete(key)
  const applyHandler = buffer.getApplyHandler()
  if (!applyHandler) {
    console.warn(`[${buffer.label}] flush skipped: no apply handler registered`)
    return
  }
  const parts = key.split('\0')
  applyHandler(parts[0], parts[1] || undefined, parts[2] || undefined, batch)
}

function enqueueToolDelta(
  buffer: ToolDeltaBuffer,
  messageId: string,
  toolCallId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  const key = toolBufferKey(messageId, toolCallId, traceId, scopedMessageId)
  buffer.pending.set(key, (buffer.pending.get(key) ?? '') + text)
  if (buffer.timers.has(key)) return
  buffer.timers.set(key, setTimeout(() => flushToolDeltaKey(buffer, key), batchMs))
}

function flushToolDeltaKey(buffer: ToolDeltaBuffer, key: string): void {
  const timer = buffer.timers.get(key)
  if (timer != null) {
    clearTimeout(timer)
    buffer.timers.delete(key)
  }
  const batch = buffer.pending.get(key)
  if (!batch) return
  buffer.pending.delete(key)
  const applyHandler = buffer.getApplyHandler()
  if (!applyHandler) {
    console.warn(`[${buffer.label}] flush skipped: no apply handler registered`)
    return
  }
  const parts = key.split('\0')
  applyHandler(parts[0], parts[1], parts[2] || undefined, parts[3] || undefined, batch)
}

function flushDeltaBuffer(buffer: DeltaBuffer, messageId?: string): void {
  const keys = new Set([...buffer.pending.keys(), ...buffer.timers.keys()])
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    flushDeltaKey(buffer, key)
  }
}

function flushToolDeltaBuffer(buffer: ToolDeltaBuffer, messageId?: string): void {
  const keys = new Set([...buffer.pending.keys(), ...buffer.timers.keys()])
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    flushToolDeltaKey(buffer, key)
  }
}

function clearDeltaBuffer(buffer: DeltaBuffer): void {
  for (const timer of buffer.timers.values()) clearTimeout(timer)
  buffer.timers.clear()
  buffer.pending.clear()
}

function clearToolDeltaBuffer(buffer: ToolDeltaBuffer): void {
  for (const timer of buffer.timers.values()) clearTimeout(timer)
  buffer.timers.clear()
  buffer.pending.clear()
}

const reasoningBuffer: DeltaBuffer = {
  pending: reasoningPending,
  timers: reasoningTimers,
  getApplyHandler: () => reasoningApplyHandler,
  label: 'reasoningDeltaBatch'
}
const contentBuffer: DeltaBuffer = {
  pending: contentPending,
  timers: contentTimers,
  getApplyHandler: () => contentApplyHandler,
  label: 'contentDeltaBatch'
}
const toolArgsBuffer: ToolDeltaBuffer = {
  pending: toolArgsPending,
  timers: toolArgsTimers,
  getApplyHandler: () => toolArgsApplyHandler,
  label: 'toolArgsDeltaBatch'
}
const toolOutputBuffer: ToolDeltaBuffer = {
  pending: toolOutputPending,
  timers: toolOutputTimers,
  getApplyHandler: () => toolOutputApplyHandler,
  label: 'toolOutputDeltaBatch'
}
const webSearchOutputBuffer: ToolDeltaBuffer = {
  pending: webSearchOutputPending,
  timers: webSearchOutputTimers,
  getApplyHandler: () => webSearchOutputApplyHandler,
  label: 'webSearchOutputDeltaBatch'
}

export function enqueueReasoningDelta(
  messageId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = REASONING_DELTA_BATCH_MS
): void {
  enqueueDelta(reasoningBuffer, messageId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueContentDelta(
  messageId: string,
  text: string,
  batchMs: number = CONTENT_DELTA_BATCH_MS
): void {
  enqueueDelta(contentBuffer, messageId, text, undefined, undefined, batchMs)
}

export function enqueueToolArgsDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = TOOL_ARGS_DELTA_BATCH_MS
): void {
  enqueueToolDelta(toolArgsBuffer, messageId, toolCallId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueToolOutputDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = TOOL_OUTPUT_DELTA_BATCH_MS
): void {
  enqueueToolDelta(toolOutputBuffer, messageId, toolCallId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueWebSearchOutputDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = WEB_SEARCH_OUTPUT_DELTA_BATCH_MS
): void {
  enqueueToolDelta(webSearchOutputBuffer, messageId, toolCallId, text, traceId, scopedMessageId, batchMs)
}

/** Flush pending reasoning for one message (all traces) or the entire buffer. */
export function flushReasoningDeltaBuffer(messageId?: string): void {
  flushDeltaBuffer(reasoningBuffer, messageId)
}

/** Flush pending assistant body text for one message or the entire buffer. */
export function flushContentDeltaBuffer(messageId?: string): void {
  flushDeltaBuffer(contentBuffer, messageId)
}

/** Flush pending tool call args for one message or the entire buffer. */
export function flushToolArgsDeltaBuffer(messageId?: string): void {
  flushToolDeltaBuffer(toolArgsBuffer, messageId)
}

/** Flush pending terminal output for one message or the entire buffer. */
export function flushToolOutputDeltaBuffer(messageId?: string): void {
  flushToolDeltaBuffer(toolOutputBuffer, messageId)
}

/** Flush pending web search output for one message or the entire buffer. */
export function flushWebSearchOutputDeltaBuffer(messageId?: string): void {
  flushToolDeltaBuffer(webSearchOutputBuffer, messageId)
}

export function flushStreamDeltaBuffers(messageId?: string): void {
  flushReasoningDeltaBuffer(messageId)
  flushContentDeltaBuffer(messageId)
  flushToolArgsDeltaBuffer(messageId)
  flushToolOutputDeltaBuffer(messageId)
  flushWebSearchOutputDeltaBuffer(messageId)
}

export function clearReasoningDeltaBuffer(): void {
  clearDeltaBuffer(reasoningBuffer)
}

export function clearContentDeltaBuffer(): void {
  clearDeltaBuffer(contentBuffer)
}

export function clearToolArgsDeltaBuffer(): void {
  clearToolDeltaBuffer(toolArgsBuffer)
}

/**
 * Drop pending batched args for one tool call without applying them.
 * Needed when a later `tool_call_start` carries the authoritative full
 * `arguments` string — otherwise a deferred flush would append chunks that
 * were already included in that snapshot (duplicate / trailing junk).
 */
export function clearToolArgsDeltaBufferForTool(
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): void {
  const key = toolBufferKey(messageId, toolCallId, traceId, scopedMessageId)
  const timer = toolArgsTimers.get(key)
  if (timer) {
    clearTimeout(timer)
    toolArgsTimers.delete(key)
  }
  toolArgsPending.delete(key)
}

export function clearToolOutputDeltaBuffer(): void {
  clearToolDeltaBuffer(toolOutputBuffer)
}

export function clearWebSearchOutputDeltaBuffer(): void {
  clearToolDeltaBuffer(webSearchOutputBuffer)
}

export function clearStreamDeltaBuffers(): void {
  clearReasoningDeltaBuffer()
  clearContentDeltaBuffer()
  clearToolArgsDeltaBuffer()
  clearToolOutputDeltaBuffer()
  clearWebSearchOutputDeltaBuffer()
}
