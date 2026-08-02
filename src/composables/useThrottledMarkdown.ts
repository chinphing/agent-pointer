import { onScopeDispose, ref, watch, type Ref } from 'vue'

export const STREAMING_MARKDOWN_THROTTLE_MS = 100
export const LONG_STREAMING_MARKDOWN_THROTTLE_MS = 250
export const LONG_SOURCE_THRESHOLD = 8000

export type ThrottledMarkdownOptions = {
  /** Throttle interval while streaming (ms). Default 100. */
  delayMs?: number
  /** Source length above which the longer interval is used. Default 8000. */
  longSourceThreshold?: number
  /** Throttle interval when source exceeds the threshold (ms). Default 250. */
  longStreamingInterval?: number
}

/** Parse immediately when idle/terminal, but cap expensive Markdown work during streaming. */
export function useThrottledMarkdown(
  source: () => string,
  streaming: () => boolean,
  parse: (source: string) => string,
  optionsOrDelay?: ThrottledMarkdownOptions | number
): Ref<string> {
  const options: ThrottledMarkdownOptions =
    typeof optionsOrDelay === 'number'
      ? { delayMs: optionsOrDelay }
      : optionsOrDelay ?? {}
  const baseDelay = options.delayMs ?? STREAMING_MARKDOWN_THROTTLE_MS
  const longThreshold = options.longSourceThreshold ?? LONG_SOURCE_THRESHOLD
  const longDelay = options.longStreamingInterval ?? LONG_STREAMING_MARKDOWN_THROTTLE_MS

  const html = ref(parse(source()))
  let timer: ReturnType<typeof setTimeout> | null = null
  let pendingSource = source()

  function clearTimer() {
    if (timer == null) return
    clearTimeout(timer)
    timer = null
  }

  function renderPending() {
    clearTimer()
    const next = parse(pendingSource)
    // Identical HTML (e.g. stable streaming chart placeholder) — skip v-html churn.
    if (next === html.value) return
    html.value = next
  }

  watch(
    [source, streaming],
    ([nextSource, isStreaming]) => {
      pendingSource = nextSource
      if (!isStreaming) {
        renderPending()
        return
      }
      if (timer != null) return
      const delay = nextSource.length > longThreshold ? longDelay : baseDelay
      timer = setTimeout(renderPending, delay)
    },
    { flush: 'sync' }
  )

  onScopeDispose(clearTimer)
  return html
}
