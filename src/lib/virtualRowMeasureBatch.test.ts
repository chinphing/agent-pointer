import { describe, expect, it, vi } from 'vitest'
import { Virtualizer, elementScroll } from '@tanstack/virtual-core'
import { messageVirtualizerBaseOptions } from './messageVirtualization'
import {
  createVirtualRowMeasureBatch,
  type VirtualRowMeasureFrameHost
} from './virtualRowMeasureBatch'

/** Manual frame clock: nothing runs until `flush()`. */
function createFrameHost() {
  const callbacks = new Map<number, () => void>()
  let nextHandle = 1
  return {
    request: vi.fn((callback: () => void) => {
      const handle = nextHandle
      nextHandle += 1
      callbacks.set(handle, callback)
      return handle
    }),
    cancel: vi.fn((handle: number) => {
      callbacks.delete(handle)
    }),
    flush() {
      const pending = [...callbacks.values()]
      callbacks.clear()
      for (const callback of pending) callback()
    },
    pendingCount() {
      return callbacks.size
    }
  } satisfies VirtualRowMeasureFrameHost & {
    flush(): void
    pendingCount(): number
  }
}

/**
 * Row element stand-in. The batch only reads the index attribute, the
 * connection flag and the height, and every height read is logged so a test can
 * prove when layout was touched.
 */
class FakeRow {
  index: number
  height: number
  connected = true
  private readonly log: string[]

  constructor(log: string[], index: number, height: number) {
    this.log = log
    this.index = index
    this.height = height
  }

  get isConnected() {
    return this.connected
  }

  get offsetHeight() {
    this.log.push(`read:${this.index}`)
    return this.height
  }

  getAttribute(name: string) {
    return name === 'data-index' ? String(this.index) : null
  }
}

function asRowElement(row: FakeRow): HTMLDivElement {
  return row as unknown as HTMLDivElement
}

describe('virtual row measure batch', () => {
  it('collects a whole patch before reading and reads every row before applying any size', () => {
    const host = createFrameHost()
    const log: string[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      applySizes(measurements) {
        for (const measurement of measurements) {
          log.push(`write:${measurement.index}:${measurement.size}`)
        }
      }
    })

    batch.register(asRowElement(new FakeRow(log, 0, 100)))
    batch.register(asRowElement(new FakeRow(log, 1, 220)))
    batch.register(asRowElement(new FakeRow(log, 2, 140)))

    // Nothing is read or applied while the patch is still registering rows.
    expect(log).toEqual([])
    expect(host.request).toHaveBeenCalledTimes(1)
    expect(batch.pending).toBe(true)

    host.flush()

    expect(log).toEqual([
      'read:0',
      'read:1',
      'read:2',
      'write:0:100',
      'write:1:220',
      'write:2:140'
    ])
    expect(batch.pending).toBe(false)
  })

  it('coalesces every registration of one frame into a single flush', () => {
    const host = createFrameHost()
    const log: string[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({ host, applySizes: () => {} })

    for (let index = 0; index < 5; index += 1) {
      batch.register(asRowElement(new FakeRow(log, index, 100 + index)))
    }

    expect(host.request).toHaveBeenCalledTimes(1)
    host.flush()

    expect(log).toEqual(['read:0', 'read:1', 'read:2', 'read:3', 'read:4'])
    // A later patch opens a new frame instead of staying latched to the flush.
    batch.register(asRowElement(new FakeRow(log, 0, 90)))
    expect(host.request).toHaveBeenCalledTimes(2)
  })

  it('keeps the newest element when the same row is registered twice in one patch', () => {
    const host = createFrameHost()
    const log: string[] = []
    const applied: Array<{ index: number; element: HTMLDivElement; size: number }> = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      applySizes(measurements) {
        applied.push(...measurements)
      }
    })

    const stale = asRowElement(new FakeRow(log, 3, 120))
    const current = asRowElement(new FakeRow(log, 3, 260))
    batch.register(stale)
    batch.register(current)
    host.flush()

    expect(log).toEqual(['read:3'])
    expect(applied).toEqual([{ index: 3, element: current, size: 260 }])
  })

  it('ignores a null registration and an element without an index attribute', () => {
    const host = createFrameHost()
    const log: string[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({ host, applySizes: () => {} })
    const indexless = { getAttribute: () => null, isConnected: true } as unknown as HTMLDivElement

    batch.register(null)
    batch.register(indexless)

    expect(host.request).not.toHaveBeenCalled()
    expect(batch.pending).toBe(false)

    host.flush()
    expect(log).toEqual([])
  })

  it('skips a row that left the window before the flush', () => {
    const host = createFrameHost()
    const log: string[] = []
    const applied: number[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      applySizes(measurements) {
        applied.push(...measurements.map(measurement => measurement.index))
      }
    })

    const departed = new FakeRow(log, 0, 100)
    batch.register(asRowElement(departed))
    batch.register(asRowElement(new FakeRow(log, 1, 200)))
    departed.connected = false

    host.flush()

    expect(log).toEqual(['read:1'])
    expect(applied).toEqual([1])
  })

  it('skips an element the range reused for another row', () => {
    const host = createFrameHost()
    const log: string[] = []
    const applied: number[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      applySizes(measurements) {
        applied.push(...measurements.map(measurement => measurement.index))
      }
    })

    const reused = new FakeRow(log, 4, 100)
    batch.register(asRowElement(reused))
    batch.register(asRowElement(new FakeRow(log, 5, 200)))
    // The row left the window and its element now renders index 7 instead.
    reused.index = 7

    host.flush()

    expect(log).toEqual(['read:5'])
    expect(applied).toEqual([5])
  })

  it('drops the pending flush on cancel', () => {
    const host = createFrameHost()
    const log: string[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({ host, applySizes: () => {} })

    batch.register(asRowElement(new FakeRow(log, 0, 100)))
    batch.cancel()

    expect(host.cancel).toHaveBeenCalledTimes(1)
    expect(batch.pending).toBe(false)

    host.flush()
    expect(log).toEqual([])
  })

  it('wraps one flush with wrapFlush', () => {
    const host = createFrameHost()
    const log: string[] = []
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      wrapFlush(flush) {
        log.push('marker:begin')
        flush()
        log.push('marker:end')
      },
      applySizes(measurements) {
        for (const measurement of measurements) log.push(`write:${measurement.index}`)
      }
    })

    batch.register(asRowElement(new FakeRow(log, 0, 100)))
    batch.register(asRowElement(new FakeRow(log, 1, 200)))
    host.flush()

    expect(log).toEqual([
      'marker:begin',
      'read:0',
      'read:1',
      'write:0',
      'write:1',
      'marker:end'
    ])
  })

  it('applies the read heights to a real virtualizer without a read in the write pass', () => {
    const host = createFrameHost()
    const log: string[] = []
    let virtualizer!: Virtualizer<HTMLElement, HTMLDivElement>
    const batch = createVirtualRowMeasureBatch<HTMLDivElement>({
      host,
      applySizes(measurements) {
        for (const measurement of measurements) virtualizer.measureElement(measurement.element)
      }
    })

    virtualizer = new Virtualizer<HTMLElement, HTMLDivElement>({
      ...messageVirtualizerBaseOptions(3, index => `turn-${index}`),
      getScrollElement: () => null,
      scrollToFn: elementScroll,
      observeElementRect: (_instance, callback) => {
        callback({ width: 800, height: 600 })
        return () => {}
      },
      observeElementOffset: (_instance, callback) => {
        callback(0, false)
        return () => {}
      },
      initialRect: { width: 800, height: 600 },
      initialOffset: 0,
      measureElement: batch.measureElementOption
    })
    virtualizer._didMount()
    virtualizer._willUpdate()
    // The render reads the virtual items before it mounts any row, which is
    // what builds the measurement cache `resizeItem` writes into.
    virtualizer.getVirtualItems()
    const resizeItem = virtualizer.resizeItem
    virtualizer.resizeItem = (index, size) => {
      log.push(`write:${index}:${size}`)
      resizeItem(index, size)
    }

    batch.register(asRowElement(new FakeRow(log, 0, 100)))
    batch.register(asRowElement(new FakeRow(log, 1, 220)))
    batch.register(asRowElement(new FakeRow(log, 2, 140)))
    host.flush()

    // Every height read precedes every virtualizer write, and the write pass
    // reuses the read heights instead of forcing layout again.
    expect(log).toEqual([
      'read:0',
      'read:1',
      'read:2',
      'write:0:100',
      'write:1:220',
      'write:2:140'
    ])
    expect(virtualizer.getVirtualItems().map(item => item.size)).toEqual([100, 220, 140])
  })
})
