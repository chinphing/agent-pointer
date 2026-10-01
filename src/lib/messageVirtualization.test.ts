import { describe, expect, it } from 'vitest'
import { Virtualizer, elementScroll } from '@tanstack/virtual-core'
import {
  MESSAGE_VIRTUAL_OVERSCAN,
  MESSAGE_VIRTUAL_PADDING_END,
  MESSAGE_VIRTUAL_PADDING_START,
  MESSAGE_VIRTUAL_ROW_ESTIMATE,
  messageRowSpacingPixels,
  messageTurnSpacingPixels,
  messageVirtualizerBaseOptions
} from './messageVirtualization'

function createVirtualizer(offset: number, count = 1000) {
  const virtualizer = new Virtualizer<HTMLElement, HTMLElement>({
    ...messageVirtualizerBaseOptions(count, index => `message-${index}`),
    getScrollElement: () => null,
    scrollToFn: elementScroll,
    observeElementRect: (_instance, callback) => {
      callback({ width: 800, height: 600 })
      return () => {}
    },
    observeElementOffset: (_instance, callback) => {
      callback(offset, false)
      return () => {}
    },
    initialRect: { width: 800, height: 600 },
    initialOffset: offset
  })
  virtualizer._didMount()
  virtualizer._willUpdate()
  return virtualizer
}

describe('message virtualization', () => {
  it('keeps the mounted window bounded around a distant scroll offset', () => {
    const virtualizer = createVirtualizer(90_000)
    const items = virtualizer.getVirtualItems()

    expect(items.length).toBeLessThan(30)
    expect(items[0]!.index).toBeGreaterThan(450)
    expect(items[items.length - 1]!.index).toBeLessThan(550)
    expect(virtualizer.getTotalSize()).toBe(
      1000 * MESSAGE_VIRTUAL_ROW_ESTIMATE
        + MESSAGE_VIRTUAL_PADDING_START
        + MESSAGE_VIRTUAL_PADDING_END
    )
  })

  it('estimates every row with the fixed constant', () => {
    const options = messageVirtualizerBaseOptions(3, index => `message-${index}`)

    // The estimate takes no index and reads no measurement — it is the constant,
    // so no measured height can reach it.
    expect(options.estimateSize()).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
    expect(options.estimateSize()).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
  })

  it('uses stable keys and the configured overscan', () => {
    const options = messageVirtualizerBaseOptions(3, index => `message-${index}`)

    expect(options.count).toBe(3)
    expect(options.getItemKey(2)).toBe('message-2')
    expect(options.overscan).toBe(MESSAGE_VIRTUAL_OVERSCAN)
  })

  it('preserves measured rows when a newly sent message extends the list', () => {
    const virtualizer = createVirtualizer(0, 3)
    virtualizer.getVirtualItems()
    virtualizer.resizeItem(0, 100)
    virtualizer.resizeItem(1, 320)
    virtualizer.resizeItem(2, 140)

    virtualizer.setOptions({ ...virtualizer.options, count: 4 })

    expect(virtualizer.getVirtualItems().slice(0, 3).map(item => item.size)).toEqual([
      100,
      320,
      140
    ])
    expect(virtualizer.getTotalSize()).toBe(
      100 + 320 + 140 + MESSAGE_VIRTUAL_ROW_ESTIMATE
        + MESSAGE_VIRTUAL_PADDING_START
        + MESSAGE_VIRTUAL_PADDING_END
    )
  })

  it('resizes only the expanded turn without resetting neighboring row measurements', () => {
    const virtualizer = createVirtualizer(0, 3)
    virtualizer.getVirtualItems()
    virtualizer.resizeItem(0, 100)
    virtualizer.resizeItem(1, 120)
    virtualizer.resizeItem(2, 140)

    virtualizer.resizeItem(1, 420)

    const measurements = virtualizer.getVirtualItems()
    expect(measurements.map(item => item.size)).toEqual([100, 420, 140])
    expect(virtualizer.getTotalSize()).toBe(
      100 + 420 + 140 + MESSAGE_VIRTUAL_PADDING_START + MESSAGE_VIRTUAL_PADDING_END
    )
  })

  it('adds separation before every turn after the first', () => {
    expect(messageTurnSpacingPixels(0)).toBe(0)
    expect(messageTurnSpacingPixels(1)).toBe(28)
    expect(messageTurnSpacingPixels(4)).toBe(28)
  })

  it('converts existing message spacing classes into measured row padding', () => {
    expect(messageRowSpacingPixels('mt-7')).toBe(28)
    expect(messageRowSpacingPixels('mt-3.5')).toBe(14)
    expect(messageRowSpacingPixels('mt-1.5')).toBe(6)
    expect(messageRowSpacingPixels('mt-0')).toBe(0)
  })
})
