// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { measureRenderedRowSlack, type RowSlackContainerLike } from './virtualRowSlack'
import { ROW_INDEX_ATTRIBUTE } from './virtualRowMeasureBatch'

/**
 * Container double: only the rows it is handed exist, so a row missing from the
 * list stands for one whose element is not in the DOM.
 */
function containerOf(rows: Array<{ index: number; height: number }>): RowSlackContainerLike {
  return {
    querySelectorAll: () =>
      rows.map(row => ({
        getAttribute: (name: string) =>
          name === ROW_INDEX_ATTRIBUTE ? String(row.index) : null,
        offsetHeight: row.height
      }))
  }
}

/** `estimateSize` for the message virtualizer — what an unmeasured row assumes. */
const ESTIMATE = 180

describe('measureRenderedRowSlack', () => {
  it('sums slack for rows placed lower than they need and overlap for taller ones', () => {
    const rows = [
      { index: 0, start: 0, end: ESTIMATE },
      { index: 1, start: ESTIMATE, end: ESTIMATE * 2 },
      { index: 2, start: ESTIMATE * 2, end: ESTIMATE * 3 }
    ]

    const sums = measureRenderedRowSlack(
      containerOf([
        { index: 0, height: 60 }, // 180 assumed vs 60 real → 120 px of blank stripe
        { index: 1, height: 400 }, // 180 assumed vs 400 real → 220 px of overlap
        { index: 2, height: 500 } // 180 assumed vs 500 real → 320 px of overlap
      ]),
      rows
    )

    expect(sums).toEqual({ slack: 120, overlap: 540 })
  })

  it('counts a row whose height was measured as neither slack nor overlap', () => {
    const sums = measureRenderedRowSlack(
      containerOf([{ index: 0, height: 240 }, { index: 1, height: 96 }]),
      [
        { index: 0, start: 0, end: 240 },
        { index: 1, start: 240, end: 336 }
      ]
    )

    expect(sums).toEqual({ slack: 0, overlap: 0 })
  })

  it('skips a row whose element is missing instead of reading it as zero-height', () => {
    const sums = measureRenderedRowSlack(
      // Index 1 is not in the container at all.
      containerOf([{ index: 0, height: ESTIMATE }, { index: 2, height: 60 }]),
      [
        { index: 0, start: 0, end: ESTIMATE },
        { index: 1, start: ESTIMATE, end: ESTIMATE * 2 },
        { index: 2, start: ESTIMATE * 2, end: ESTIMATE * 3 }
      ]
    )

    // Only row 2 deviates; the missing row contributes nothing either way.
    expect(sums).toEqual({ slack: 120, overlap: 0 })
  })

  it('reports nothing for an empty window, an empty container or no container', () => {
    expect(measureRenderedRowSlack(containerOf([]), [])).toEqual({ slack: 0, overlap: 0 })
    expect(measureRenderedRowSlack(containerOf([]), [{ index: 0, start: 0, end: ESTIMATE }]))
      .toEqual({ slack: 0, overlap: 0 })
    expect(measureRenderedRowSlack(containerOf([{ index: 0, height: 60 }]), []))
      .toEqual({ slack: 0, overlap: 0 })
    expect(() => measureRenderedRowSlack(null, null)).not.toThrow()
    expect(measureRenderedRowSlack(null, null)).toEqual({ slack: 0, overlap: 0 })
  })

  it('finds the rows through the data-index attribute the measure batch reads', () => {
    const host = document.createElement('div')
    for (const [index, height] of [[3, 60], [4, 400]] as const) {
      const element = document.createElement('div')
      element.setAttribute(ROW_INDEX_ATTRIBUTE, String(index))
      Object.defineProperty(element, 'offsetHeight', { value: height })
      host.appendChild(element)
    }
    // An element with an unparsable index must not be counted as a row.
    const unparsable = document.createElement('div')
    unparsable.setAttribute(ROW_INDEX_ATTRIBUTE, 'not-a-number')
    host.appendChild(unparsable)

    const sums = measureRenderedRowSlack(host, [
      { index: 3, start: 0, end: ESTIMATE },
      { index: 4, start: ESTIMATE, end: ESTIMATE * 2 },
      // Rendered range said 5 is visible, but its element is gone.
      { index: 5, start: ESTIMATE * 2, end: ESTIMATE * 3 }
    ])

    expect(sums).toEqual({ slack: 120, overlap: 220 })
  })
})
