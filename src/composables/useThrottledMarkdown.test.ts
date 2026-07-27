import { effectScope, ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  LONG_SOURCE_THRESHOLD,
  LONG_STREAMING_MARKDOWN_THROTTLE_MS,
  STREAMING_MARKDOWN_THROTTLE_MS,
  useThrottledMarkdown
} from './useThrottledMarkdown'

afterEach(() => {
  vi.useRealTimers()
})

describe('useThrottledMarkdown', () => {
  it('coalesces streaming updates and flushes the terminal source immediately', () => {
    vi.useFakeTimers()
    const scope = effectScope()
    const source = ref('a')
    const streaming = ref(true)
    const parse = vi.fn((value: string) => `<p>${value}</p>`)

    const html = scope.run(() => useThrottledMarkdown(
      () => source.value,
      () => streaming.value,
      parse
    ))!

    source.value = 'ab'
    source.value = 'abc'
    expect(parse).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(STREAMING_MARKDOWN_THROTTLE_MS - 1)
    expect(parse).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(html.value).toBe('<p>abc</p>')
    expect(parse).toHaveBeenCalledTimes(2)

    source.value = 'final'
    streaming.value = false
    expect(html.value).toBe('<p>final</p>')
    scope.stop()
  })

  it('uses the longer interval once the source exceeds the length threshold', () => {
    vi.useFakeTimers()
    const scope = effectScope()
    const source = ref('a'.repeat(LONG_SOURCE_THRESHOLD + 1))
    const streaming = ref(true)
    const parse = vi.fn((value: string) => `<p>${value.length}</p>`)

    const html = scope.run(() => useThrottledMarkdown(
      () => source.value,
      () => streaming.value,
      parse
    ))!

    source.value = `${source.value}b`
    expect(parse).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(LONG_STREAMING_MARKDOWN_THROTTLE_MS - 1)
    expect(parse).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(html.value).toBe(`<p>${LONG_SOURCE_THRESHOLD + 2}</p>`)
    expect(parse).toHaveBeenCalledTimes(2)
    scope.stop()
  })
})
