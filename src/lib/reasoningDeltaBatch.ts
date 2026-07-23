/** Batch reactive UI writes for streamed text to reduce Vue re-render churn. */

export const REASONING_DELTA_BATCH_MS = 200
export const CONTENT_DELTA_BATCH_MS = 50

export type StreamDeltaApply = (
  messageId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  text: string
) => void

export type ReasoningDeltaApply = StreamDeltaApply
export type ContentDeltaApply = StreamDeltaApply

type DeltaBuffer = {
  pending: Map<string, string>
  timers: Map<string, ReturnType<typeof setTimeout>>
  getApplyHandler: () => StreamDeltaApply | null
  label: string
}

const reasoningPending = new Map<string, string>()
const reasoningTimers = new Map<string, ReturnType<typeof setTimeout>>()
const contentPending = new Map<string, string>()
const contentTimers = new Map<string, ReturnType<typeof setTimeout>>()

let reasoningApplyHandler: ReasoningDeltaApply | null = null
let contentApplyHandler: ContentDeltaApply | null = null

function bufferKey(messageId: string, traceId?: string, scopedMessageId?: string): string {
  const tid = traceId?.trim()
  const sid = scopedMessageId?.trim()
  if (sid) return `${messageId}\0${tid ?? ''}\0${sid}`
  if (tid) return `${messageId}\0${tid}`
  return messageId
}

export function setReasoningDeltaApplyHandler(handler: ReasoningDeltaApply | null): void {
  reasoningApplyHandler = handler
}

export function setContentDeltaApplyHandler(handler: ContentDeltaApply | null): void {
  contentApplyHandler = handler
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

function flushDeltaBuffer(buffer: DeltaBuffer, messageId?: string): void {
  const keys = new Set([...buffer.pending.keys(), ...buffer.timers.keys()])
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    flushDeltaKey(buffer, key)
  }
}

function clearDeltaBuffer(buffer: DeltaBuffer): void {
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

/** Flush pending reasoning for one message (all traces) or the entire buffer. */
export function flushReasoningDeltaBuffer(messageId?: string): void {
  flushDeltaBuffer(reasoningBuffer, messageId)
}

/** Flush pending assistant body text for one message or the entire buffer. */
export function flushContentDeltaBuffer(messageId?: string): void {
  flushDeltaBuffer(contentBuffer, messageId)
}

export function flushStreamDeltaBuffers(messageId?: string): void {
  flushReasoningDeltaBuffer(messageId)
  flushContentDeltaBuffer(messageId)
}

export function clearReasoningDeltaBuffer(): void {
  clearDeltaBuffer(reasoningBuffer)
}

export function clearContentDeltaBuffer(): void {
  clearDeltaBuffer(contentBuffer)
}

export function clearStreamDeltaBuffers(): void {
  clearReasoningDeltaBuffer()
  clearContentDeltaBuffer()
}
