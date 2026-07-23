import { effectScope, ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useThrottledMarkdown } from './useThrottledMarkdown'

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
      parse,
      100
    ))!

    source.value = 'ab'
    source.value = 'abc'
    expect(parse).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(100)
    expect(html.value).toBe('<p>abc</p>')
    expect(parse).toHaveBeenCalledTimes(2)

    source.value = 'final'
    streaming.value = false
    expect(html.value).toBe('<p>final</p>')
    scope.stop()
  })
})
