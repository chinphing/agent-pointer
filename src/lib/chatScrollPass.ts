/**
 * Scheduling primitives for the `MessageList` scroll pass.
 *
 * A scroll event fires several times per animation frame, and the pass is not
 * free: it reads layout (`scrollTop` / `scrollHeight` / `clientHeight`, the
 * task-board rect) and then writes refs and store state. Running it per event
 * repeats those reads and interleaves them with the writes.
 *
 * - `createScrollPassScheduler` keeps it to one pass per frame.
 * - `createViewedStampDedupe` keeps the "user message seen in the viewport"
 *   store writes to the rows that actually entered the viewport.
 */

/** Injectable frame clock so the coalescing can be tested without a real rAF. */
export interface ScrollPassFrameHost {
  request(callback: () => void): number
  cancel(handle: number): void
}

const browserFrameHost: ScrollPassFrameHost = {
  request: callback => requestAnimationFrame(callback),
  cancel: handle => cancelAnimationFrame(handle)
}

export interface ScrollPassScheduler {
  /** Run the pass on the next frame; further calls inside that frame coalesce. */
  schedule(): void
  /** Drop a scheduled pass that has not run yet. */
  cancel(): void
  readonly pending: boolean
}

/**
 * Coalesce every `schedule()` inside one animation frame into a single `run()`.
 * The pass starts when the frame fires, so it always sees the state the newest
 * scroll event left behind.
 */
export function createScrollPassScheduler(
  run: () => void,
  host: ScrollPassFrameHost = browserFrameHost
): ScrollPassScheduler {
  let handle: number | null = null
  return {
    schedule() {
      if (handle != null) return
      handle = host.request(() => {
        handle = null
        run()
      })
    },
    cancel() {
      if (handle == null) return
      host.cancel(handle)
      handle = null
    },
    get pending() {
      return handle != null
    }
  }
}

export interface ViewedStampDedupe {
  /**
   * Stamp `visibleIds` through `stamp`, skipping the ids the previous call for
   * the same conversation already stamped. A conversation change forgets the
   * remembered set.
   */
  apply(
    conversationId: string,
    visibleIds: readonly string[],
    stamp: (messageId: string) => void
  ): void
  /** Forget the previous call so the next `apply` stamps every id again. */
  reset(): void
}

/**
 * Keep the viewport "viewed" stamps from repeating for rows the previous pass
 * already stamped. Ids that leave and re-enter the viewport are stamped again,
 * which matches the store's freshness renewal: `trimConversationHistory` drops
 * the viewed record of every row it removes.
 */
export function createViewedStampDedupe(): ViewedStampDedupe {
  let conversationId: string | null = null
  let stampedIds = new Set<string>()
  return {
    apply(nextConversationId, visibleIds, stamp) {
      if (nextConversationId !== conversationId) {
        conversationId = nextConversationId
        stampedIds = new Set()
      }
      const nextStampedIds = new Set<string>()
      for (const id of visibleIds) {
        nextStampedIds.add(id)
        if (stampedIds.has(id)) continue
        stamp(id)
      }
      stampedIds = nextStampedIds
    },
    reset() {
      conversationId = null
      stampedIds = new Set()
    }
  }
}
