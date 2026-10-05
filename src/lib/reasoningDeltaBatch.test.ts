import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ASSISTANT_JSON_PARTIAL_BATCH_MS,
  clearStreamDeltaBuffers,
  clearToolArgsDeltaBufferForTool,
  CONTENT_DELTA_BATCH_MS,
  contentLaneFor,
  enqueueAssistantJsonPartial,
  enqueueContentDelta,
  enqueueReasoningDelta,
  enqueueToolArgsDelta,
  enqueueToolOutputDelta,
  enqueueWebSearchOutputDelta,
  flushAssistantJsonPartialBuffer,
  flushContentDeltaBuffer,
  flushStreamDeltaBuffers,
  HIGH_PRESSURE_BATCH_MS,
  HIGH_PRESSURE_STREAM_KEYS,
  pendingDeltaCount,
  REASONING_DELTA_BATCH_MS,
  setAssistantJsonPartialApplyHandler,
  setContentDeltaApplyHandler,
  setReasoningDeltaApplyHandler,
  setScopedMessageIdPredicate,
  setToolArgsDeltaApplyHandler,
  setToolOutputDeltaApplyHandler,
  setWebSearchOutputDeltaApplyHandler,
  SLOW_LANE_BATCH_MS,
  streamLaneFor,
  TOOL_ARGS_DELTA_BATCH_MS,
  TOOL_OUTPUT_DELTA_BATCH_MS,
  WEB_SEARCH_OUTPUT_DELTA_BATCH_MS
} from './reasoningDeltaBatch'

afterEach(() => {
  clearStreamDeltaBuffers()
  setContentDeltaApplyHandler(null)
  setReasoningDeltaApplyHandler(null)
  setToolArgsDeltaApplyHandler(null)
  setToolOutputDeltaApplyHandler(null)
  setWebSearchOutputDeltaApplyHandler(null)
  setAssistantJsonPartialApplyHandler(null)
  setScopedMessageIdPredicate(null)
  vi.useRealTimers()
})

describe('content delta batching', () => {
  it('coalesces deltas in order and supports synchronous terminal flush', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'hel')
    enqueueContentDelta('m1', 'lo')
    expect(apply).not.toHaveBeenCalled()

    flushContentDeltaBuffer('m1')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, 'hello')
    vi.runAllTimers()
    expect(apply).toHaveBeenCalledTimes(1)
  })

  it('keeps message buffers isolated', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'one')
    enqueueContentDelta('m2', 'two')
    flushContentDeltaBuffer('m1')

    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, 'one')
    expect(apply).not.toHaveBeenCalledWith('m2', undefined, undefined, 'two')
    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledWith('m2', undefined, undefined, 'two')
  })

  it('flushes all pending keys in one shared timer tick', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'a')
    enqueueContentDelta('m2', 'b')
    enqueueContentDelta('m3', 'c')
    expect(apply).not.toHaveBeenCalled()

    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(3)
  })
})

describe('assistant json partial batching', () => {
  it('merges latest-wins fields and flushes once', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setAssistantJsonPartialApplyHandler(apply)

    enqueueAssistantJsonPartial('m1', { thoughts: 'a', toolName: 'search' })
    enqueueAssistantJsonPartial('m1', { thoughts: 'ab', responseText: 'hi' })
    expect(apply).not.toHaveBeenCalled()

    flushAssistantJsonPartialBuffer('m1')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, {
      thoughts: 'ab',
      toolName: 'search',
      responseText: 'hi'
    })
  })
})

describe('tool args delta batching', () => {
  it('discards pending args when an authoritative snapshot arrives', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setToolArgsDeltaApplyHandler(apply)

    enqueueToolArgsDelta('m1', 'tc1', '}')
    clearToolArgsDeltaBufferForTool('m1', 'tc1')
    vi.advanceTimersByTime(100)

    expect(apply).not.toHaveBeenCalled()
  })
})

// ---------------------------------------------------------------------------
// Fast lane — byte-for-byte the old behavior (constants and semantics untouched)
// ---------------------------------------------------------------------------

describe('fast lane keeps its existing delays', () => {
  const cases = [
    { name: 'content', ms: CONTENT_DELTA_BATCH_MS, enqueue: (apply: () => void) => {
      setContentDeltaApplyHandler(apply)
      enqueueContentDelta('lead', 'x')
    } },
    { name: 'reasoning', ms: REASONING_DELTA_BATCH_MS, enqueue: (apply: () => void) => {
      setReasoningDeltaApplyHandler(apply)
      enqueueReasoningDelta('lead', 'x')
    } },
    { name: 'toolArgs', ms: TOOL_ARGS_DELTA_BATCH_MS, enqueue: (apply: () => void) => {
      setToolArgsDeltaApplyHandler(apply)
      enqueueToolArgsDelta('lead', 'tc', 'x')
    } },
    { name: 'toolOutput', ms: TOOL_OUTPUT_DELTA_BATCH_MS, enqueue: (apply: () => void) => {
      setToolOutputDeltaApplyHandler(apply)
      enqueueToolOutputDelta('lead', 'tc', 'x')
    } },
    { name: 'webSearchOutput', ms: WEB_SEARCH_OUTPUT_DELTA_BATCH_MS, enqueue: (apply: () => void) => {
      setWebSearchOutputDeltaApplyHandler(apply)
      enqueueWebSearchOutputDelta('lead', 'tc', 'x')
    } },
    { name: 'jsonPartial', ms: ASSISTANT_JSON_PARTIAL_BATCH_MS, enqueue: (apply: () => void) => {
      setAssistantJsonPartialApplyHandler(apply)
      enqueueAssistantJsonPartial('lead', { thoughts: 'x' })
    } }
  ]

  it.each(cases)('$name waits exactly $ms ms', ({ ms, enqueue }) => {
    vi.useFakeTimers()
    const apply = vi.fn()
    enqueue(apply)

    vi.advanceTimersByTime(ms - 1)
    expect(apply).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(apply).toHaveBeenCalledTimes(1)
  })
})

// ---------------------------------------------------------------------------
// Lane routing
// ---------------------------------------------------------------------------

describe('lane routing', () => {
  it('streamLaneFor: any sub-agent identity means the slow lane', () => {
    expect(streamLaneFor(undefined, undefined)).toBe('fast')
    expect(streamLaneFor('', '  ')).toBe('fast')
    expect(streamLaneFor('task:worker', undefined)).toBe('slow')
    expect(streamLaneFor(undefined, 'row-1')).toBe('slow')
  })

  it('contentLaneFor defaults to fast with no predicate registered', () => {
    expect(contentLaneFor('m1')).toBe('fast')
    expect(contentLaneFor('')).toBe('fast')
  })

  it('contentLaneFor follows the injected predicate', () => {
    setScopedMessageIdPredicate(id => id.startsWith('row-'))
    expect(contentLaneFor('row-1')).toBe('slow')
    expect(contentLaneFor('lead-1')).toBe('fast')
  })

  it('a throwing predicate degrades to the fast lane', () => {
    setScopedMessageIdPredicate(() => {
      throw new Error('boom')
    })
    expect(contentLaneFor('m1')).toBe('fast')
  })

  it('the identity channels route sub-agent deltas to the slow lane', () => {
    vi.useFakeTimers()
    const reasoning = vi.fn()
    const toolArgs = vi.fn()
    const toolOutput = vi.fn()
    const webSearch = vi.fn()
    const jsonPartial = vi.fn()
    setReasoningDeltaApplyHandler(reasoning)
    setToolArgsDeltaApplyHandler(toolArgs)
    setToolOutputDeltaApplyHandler(toolOutput)
    setWebSearchOutputDeltaApplyHandler(webSearch)
    setAssistantJsonPartialApplyHandler(jsonPartial)

    enqueueReasoningDelta('lead', 'r', 'task:worker')
    enqueueToolArgsDelta('lead', 'tc1', 'a', undefined, 'row-1')
    enqueueToolOutputDelta('lead', 'tc2', 'o', 'task:worker')
    enqueueWebSearchOutputDelta('lead', 'tc3', 'w', undefined, 'row-1')
    enqueueAssistantJsonPartial('lead', { thoughts: 't' }, 'task:worker')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS - 1)
    expect(reasoning).not.toHaveBeenCalled()
    expect(toolArgs).not.toHaveBeenCalled()
    expect(toolOutput).not.toHaveBeenCalled()
    expect(webSearch).not.toHaveBeenCalled()
    expect(jsonPartial).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(reasoning).toHaveBeenCalledTimes(1)
    expect(toolArgs).toHaveBeenCalledTimes(1)
    expect(toolOutput).toHaveBeenCalledTimes(1)
    expect(webSearch).toHaveBeenCalledTimes(1)
    expect(jsonPartial).toHaveBeenCalledTimes(1)
  })

  it('content routes by the injected predicate and holds for the slow batch', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    enqueueContentDelta('lead', 'lead text')
    enqueueContentDelta('row-1', 'sub text')

    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('lead', undefined, undefined, 'lead text')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS - CONTENT_DELTA_BATCH_MS - 1)
    expect(apply).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(apply).toHaveBeenCalledWith('row-1', undefined, undefined, 'sub text')
  })

  it('many slow keys share one tick per second', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    for (let i = 0; i < 6; i++) enqueueContentDelta(`row-${i}`, 'x')
    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(6)
  })

  it('a slow key never falls back: an unrelated fast flush leaves it pending', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    enqueueContentDelta('row-1', 'sub text')
    enqueueContentDelta('lead', 'lead text')

    // Message-end style flush for the lead message only.
    flushContentDeltaBuffer('lead')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('lead', undefined, undefined, 'lead text')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS - 1)
    expect(apply).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(apply).toHaveBeenCalledTimes(2)
  })
})

// ---------------------------------------------------------------------------
// Per-lane high pressure
// ---------------------------------------------------------------------------

describe('per-lane high pressure', () => {
  it('slow keys do not stretch the fast lane', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    // Well past HIGH_PRESSURE_STREAM_KEYS, but all of them slow.
    for (let i = 0; i < HIGH_PRESSURE_STREAM_KEYS + 4; i++) enqueueContentDelta(`row-${i}`, 's')
    enqueueContentDelta('lead-a', 'a')
    enqueueContentDelta('lead-b', 'b')

    // A partial flush reschedules the fast lane — with the old *global* count this
    // reschedule would have been stretched to HIGH_PRESSURE_BATCH_MS.
    flushContentDeltaBuffer('lead-a')
    apply.mockClear()

    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('lead-b', undefined, undefined, 'b')
  })

  it('the fast lane still stretches under its own load', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    for (let i = 0; i < HIGH_PRESSURE_STREAM_KEYS + 2; i++) enqueueContentDelta(`lead-${i}`, 'f')
    flushContentDeltaBuffer('lead-0')
    apply.mockClear()

    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).not.toHaveBeenCalled()
    vi.advanceTimersByTime(HIGH_PRESSURE_BATCH_MS - CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(HIGH_PRESSURE_STREAM_KEYS + 1)
  })

  it('the slow lane reschedules at its own (longer) batch, never the fast stretch', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    for (let i = 0; i < HIGH_PRESSURE_STREAM_KEYS + 2; i++) enqueueContentDelta(`row-${i}`, 's')
    flushContentDeltaBuffer('row-0')
    apply.mockClear()

    vi.advanceTimersByTime(HIGH_PRESSURE_BATCH_MS)
    expect(apply).not.toHaveBeenCalled()
    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS - HIGH_PRESSURE_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(HIGH_PRESSURE_STREAM_KEYS + 1)
  })
})

// ---------------------------------------------------------------------------
// Escape valves
// ---------------------------------------------------------------------------

describe('escape valves', () => {
  it('message end / turn end flush lands slow-lane text immediately', () => {
    vi.useFakeTimers()
    const content = vi.fn()
    const reasoning = vi.fn()
    setContentDeltaApplyHandler(content)
    setReasoningDeltaApplyHandler(reasoning)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    enqueueContentDelta('row-1', 'sub text')
    enqueueReasoningDelta('lead', 'think', 'task:worker')

    flushStreamDeltaBuffers()
    expect(content).toHaveBeenCalledWith('row-1', undefined, undefined, 'sub text')
    expect(reasoning).toHaveBeenCalledWith('lead', 'task:worker', undefined, 'think')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(content).toHaveBeenCalledTimes(1)
    expect(reasoning).toHaveBeenCalledTimes(1)
  })

  it('message end flush for one message leaves other lanes pending', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    enqueueContentDelta('row-1', 'sub text')
    enqueueContentDelta('lead', 'lead text')

    flushStreamDeltaBuffers('lead')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('lead', undefined, undefined, 'lead text')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(2)
  })

  it('ask_user / approval: an authoritative snapshot drops leftovers in both lanes', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setToolArgsDeltaApplyHandler(apply)

    enqueueToolArgsDelta('m1', 'tc-fast', '{"question":"q"')
    enqueueToolArgsDelta('m1', 'tc-slow', '{"question":"q"', undefined, 'row-1')

    clearToolArgsDeltaBufferForTool('m1', 'tc-fast')
    clearToolArgsDeltaBufferForTool('m1', 'tc-slow', undefined, 'row-1')

    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(apply).not.toHaveBeenCalled()
  })
})

// ---------------------------------------------------------------------------
// HUD counters
// ---------------------------------------------------------------------------

describe('pendingDeltaCount', () => {
  it('sums both lanes and drains on the message-end flush', () => {
    const before = pendingDeltaCount()
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    enqueueContentDelta('lead', 'a')
    enqueueContentDelta('row-1', 'b')
    enqueueReasoningDelta('lead', 'c', 'task:worker')
    enqueueToolArgsDelta('lead', 'tc', 'd', undefined, 'row-2')

    expect(pendingDeltaCount()).toBe(before + 4)
    flushStreamDeltaBuffers()
    expect(pendingDeltaCount()).toBe(before)
  })

  it('drains on clear as well', () => {
    const before = pendingDeltaCount()
    setScopedMessageIdPredicate(id => id.startsWith('row-'))
    enqueueContentDelta('row-1', 'b')
    clearStreamDeltaBuffers()
    expect(pendingDeltaCount()).toBe(before)
  })
})
