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

/**
 * Expected sums when no viewport is handed in: the in-viewport split then counts every
 * band, so it can only over-report and never hide a misplacement off screen.
 */
function sumsOf(slack: number, overlap: number) {
  return { slack, overlap, slackVisible: slack, overlapVisible: overlap }
}

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

    expect(sums).toEqual(sumsOf(120, 540))
  })

  /**
   * Why a single unmeasured row is not a local error: rows are placed by the running sum
   * of sizes, so a turn that really is ~4.9k px tall but still carries the 180 px estimate
   * leaves every row below it placed ~4.7k px too high — stacked on top of it. The gauge
   * number and the placement deficit are the same quantity, which is what makes this
   * visible rather than merely reported.
   */
  it('places every row below an unmeasured tall row short by the overlap it reports', () => {
    const REAL = 4856
    // Placed as if the tall turn were one estimate: the sum, not the truth.
    const rows = [
      { index: 0, start: 0, end: ESTIMATE },
      { index: 1, start: ESTIMATE, end: ESTIMATE * 2 }
    ]

    const sums = measureRenderedRowSlack(
      containerOf([{ index: 0, height: REAL }, { index: 1, height: ESTIMATE }]),
      rows
    )

    expect(sums.overlap).toBe(REAL - ESTIMATE)
    // The row after it sits at `ESTIMATE`; its true position would be `REAL`.
    expect(rows[1]!.start).toBe(REAL - sums.overlap)
  })

  /**
   * The question a peak on the totals cannot answer: was the misplacement on screen, or
   * did it happen in the overscan band? Both are measured against the same tick's
   * viewport, so the split and `v.range` describe one instant.
   */
  it('splits the sums into the bands that were inside the viewport', () => {
    const rows = [
      { index: 0, start: 0, end: ESTIMATE },
      { index: 1, start: ESTIMATE, end: ESTIMATE * 2 },
      { index: 2, start: ESTIMATE * 2, end: ESTIMATE * 3 }
    ]
    const container = containerOf([
      { index: 0, height: ESTIMATE }, // measured: contributes nothing
      { index: 1, height: 400 }, // 180 assumed vs 400 real → 220 px band, content ends at 580
      { index: 2, height: 60 } // 180 assumed vs 60 real → 120 px band, content ends at 420
    ])

    // Viewport [180, 580): both bands are inside it.
    expect(measureRenderedRowSlack(container, rows, { scrollTop: 180, height: 400 }))
      .toEqual({ slack: 120, overlap: 220, slackVisible: 120, overlapVisible: 220 })

    // Viewport far below every band: the totals stay, the visible split is 0.
    expect(measureRenderedRowSlack(container, rows, { scrollTop: 5000, height: 600 }))
      .toEqual({ slack: 120, overlap: 220, slackVisible: 0, overlapVisible: 0 })

    // Viewport [200, 400): row 1's overlap band reaches into it, row 2's slack band does not.
    expect(measureRenderedRowSlack(container, rows, { scrollTop: 200, height: 200 }))
      .toEqual({ slack: 120, overlap: 220, slackVisible: 0, overlapVisible: 220 })
  })

  it('counts a row whose height was measured as neither slack nor overlap', () => {
    const sums = measureRenderedRowSlack(
      containerOf([{ index: 0, height: 240 }, { index: 1, height: 96 }]),
      [
        { index: 0, start: 0, end: 240 },
        { index: 1, start: 240, end: 336 }
      ]
    )

    expect(sums).toEqual(sumsOf(0, 0))
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
    expect(sums).toEqual(sumsOf(120, 0))
  })

  it('reports nothing for an empty window, an empty container or no container', () => {
    expect(measureRenderedRowSlack(containerOf([]), [])).toEqual(sumsOf(0, 0))
    expect(measureRenderedRowSlack(containerOf([]), [{ index: 0, start: 0, end: ESTIMATE }]))
      .toEqual(sumsOf(0, 0))
    expect(measureRenderedRowSlack(containerOf([{ index: 0, height: 60 }]), []))
      .toEqual(sumsOf(0, 0))
    expect(() => measureRenderedRowSlack(null, null)).not.toThrow()
    expect(measureRenderedRowSlack(null, null)).toEqual(sumsOf(0, 0))
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

    expect(sums).toEqual(sumsOf(120, 220))
  })
})
