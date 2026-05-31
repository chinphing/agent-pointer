/** Batch UI writes for `reasoning_delta` to reduce Vue re-render churn during long thinking streams. */

export const REASONING_DELTA_BATCH_MS = 200

export type ReasoningDeltaApply = (
  messageId: string,
  traceId: string | undefined,
  text: string
) => void

const pending = new Map<string, string>()
const timers = new Map<string, ReturnType<typeof setTimeout>>()

let applyHandler: ReasoningDeltaApply | null = null

function bufferKey(messageId: string, traceId?: string): string {
  const tid = traceId?.trim()
  return tid ? `${messageId}\0${tid}` : messageId
}

export function setReasoningDeltaApplyHandler(handler: ReasoningDeltaApply | null): void {
  applyHandler = handler
}

export function enqueueReasoningDelta(
  messageId: string,
  text: string,
  traceId?: string,
  batchMs: number = REASONING_DELTA_BATCH_MS
): void {
  if (!text) return
  const key = bufferKey(messageId, traceId)
  pending.set(key, (pending.get(key) ?? '') + text)
  if (timers.has(key)) return
  timers.set(
    key,
    setTimeout(() => {
      flushReasoningDeltaKey(key)
    }, batchMs)
  )
}

function flushReasoningDeltaKey(key: string): void {
  const timer = timers.get(key)
  if (timer != null) {
    clearTimeout(timer)
    timers.delete(key)
  }
  const batch = pending.get(key)
  if (!batch) return
  pending.delete(key)
  if (!applyHandler) {
    console.warn('[reasoningDeltaBatch] flush skipped: no apply handler registered')
    return
  }
  const sep = key.indexOf('\0')
  if (sep >= 0) {
    applyHandler(key.slice(0, sep), key.slice(sep + 1), batch)
  } else {
    applyHandler(key, undefined, batch)
  }
}

/** Flush pending reasoning for one message (all traces) or entire buffer. */
export function flushReasoningDeltaBuffer(messageId?: string): void {
  const keys = [...pending.keys(), ...timers.keys()]
  for (const key of keys) {
    if (messageId != null) {
      const sep = key.indexOf('\0')
      const mid = sep >= 0 ? key.slice(0, sep) : key
      if (mid !== messageId) continue
    }
    flushReasoningDeltaKey(key)
  }
}

export function clearReasoningDeltaBuffer(): void {
  for (const timer of timers.values()) clearTimeout(timer)
  timers.clear()
  pending.clear()
}
