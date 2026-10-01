import { describe, expect, it } from 'vitest'
import { Virtualizer, elementScroll } from '@tanstack/virtual-core'
import {
  MESSAGE_VIRTUAL_OVERSCAN,
  MESSAGE_VIRTUAL_PADDING_END,
  MESSAGE_VIRTUAL_PADDING_START,
  MESSAGE_VIRTUAL_ROW_ESTIMATE,
  createMessageRowHeightEstimator,
  messageRowSpacingPixels,
  messageTurnSpacingPixels,
  messageVirtualizerBaseOptions
} from './messageVirtualization'

function createVirtualizer(offset: number, count = 1000, estimateSize?: () => number) {
  const virtualizer = new Virtualizer<HTMLElement, HTMLElement>({
    ...messageVirtualizerBaseOptions(count, index => `message-${index}`, estimateSize),
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

describe('message row height estimate', () => {
  it('falls back to the fixed estimate before anything has been measured', () => {
    const estimator = createMessageRowHeightEstimator()

    expect(estimator.estimate).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)

    // An empty read pass changes nothing either.
    estimator.sample([])
    expect(estimator.estimate).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
  })

  it('follows the average of the heights a measure pass read', () => {
    const estimator = createMessageRowHeightEstimator()

    estimator.sample([{ size: 60 }, { size: 240 }])
    expect(estimator.estimate).toBe(150)

    // Unusable reads are ignored rather than dragging the average down.
    estimator.sample([{ size: 0 }, { size: Number.NaN }, { size: -20 }])
    expect(estimator.estimate).toBe(150)

    estimator.sample([{ size: 300 }, { size: 300 }])
    expect(estimator.estimate).toBe(225)
  })

  it('bounds one enormous row instead of letting it skew the whole list', () => {
    const estimator = createMessageRowHeightEstimator()

    estimator.sample(Array.from({ length: 256 }, () => ({ size: 100 })))
    expect(estimator.estimate).toBe(100)

    estimator.sample([{ size: 5000 }])

    // The window bounds one row's influence to 1/256 of its deviation: ~19 px of
    // movement, not 4900.
    expect(estimator.estimate).toBeCloseTo((255 * 100 + 5000) / 256, 5)
    expect(estimator.estimate).toBeLessThan(120)
  })

  it('passes the running estimate to every unmeasured row', () => {
    const estimator = createMessageRowHeightEstimator()
    const options = messageVirtualizerBaseOptions(
      3,
      index => `message-${index}`,
      () => estimator.estimate
    )

    expect(options.estimateSize(0)).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
    estimator.sample([{ size: 96 }])
    expect([0, 1, 2].map(index => options.estimateSize(index))).toEqual([96, 96, 96])

    // Without an estimator the fixed fallback is still what the options carry.
    expect(messageVirtualizerBaseOptions(1, index => `message-${index}`).estimateSize(0))
      .toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
  })

  it('keeps measured rows at their exact size while the estimate moves', () => {
    const estimator = createMessageRowHeightEstimator()
    const virtualizer = createVirtualizer(0, 4, () => estimator.estimate)
    const sizes = () => virtualizer.getVirtualItems().map(item => item.size)
    virtualizer.getVirtualItems()

    // The batch's order: fold the pass, then hand the read heights over.
    estimator.sample([{ size: 100 }, { size: 100 }])
    virtualizer.resizeItem(0, 500)
    expect(sizes()).toEqual([500, 100, 100, 100])

    estimator.sample([{ size: 40 }, { size: 40 }])
    virtualizer.resizeItem(1, 40)

    // The rows still unmeasured moved to the new average (70); the two measured
    // rows kept exactly the heights that were read, the 500 px one included.
    expect(estimator.estimate).toBe(70)
    expect(sizes()).toEqual([500, 40, 70, 70])
    expect(virtualizer.getTotalSize()).toBe(
      500 + 40 + 70 + 70 + MESSAGE_VIRTUAL_PADDING_START + MESSAGE_VIRTUAL_PADDING_END
    )
  })

  it('starts from the fallback again for a new conversation or a recreated virtualizer', () => {
    const estimator = createMessageRowHeightEstimator()
    estimator.sample([{ size: 60 }, { size: 60 }])
    expect(estimator.estimate).toBe(60)

    estimator.reset()
    expect(estimator.estimate).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)

    // `MessageList` is keyed by conversation id, so a switch builds a new
    // estimator instead of reusing the previous conversation's average.
    const nextConversation = createMessageRowHeightEstimator()
    expect(nextConversation.estimate).toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
  })
})
