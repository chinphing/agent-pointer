import { onScopeDispose, ref, watch, type Ref } from 'vue'

export const STREAMING_MARKDOWN_THROTTLE_MS = 100

/** Parse immediately when idle/terminal, but cap expensive Markdown work during streaming. */
export function useThrottledMarkdown(
  source: () => string,
  streaming: () => boolean,
  parse: (source: string) => string,
  delayMs: number = STREAMING_MARKDOWN_THROTTLE_MS
): Ref<string> {
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
    html.value = parse(pendingSource)
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
      timer = setTimeout(renderPending, delayMs)
    },
    { flush: 'sync' }
  )

  onScopeDispose(clearTimer)
  return html
}
