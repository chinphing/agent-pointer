import { ROW_INDEX_ATTRIBUTE } from './virtualRowMeasureBatch'

/**
 * Per-row "assumed vs actual" slack for the render-perf HUD.
 *
 * Why this exists:
 *
 * - Rows are absolutely positioned at `translateY(row.start)`, and `start` is the
 *   running sum of the row sizes. A row with no measured height contributes
 *   `estimateSize` (180 px) instead of its real height, so the *next* row starts
 *   at the wrong offset: too far down when the estimate is too big, which shows
 *   as a blank stripe between the two rows, and too far up when it is too small,
 *   which shows as an overlap.
 * - `scrollHeight - getTotalSize()` cannot see any of that. The scroll container's
 *   content height is written *from* the virtualizer's total, so the two agree by
 *   construction and their difference is only the surrounding padding. The
 *   per-row difference is where the error actually lives.
 * - So this module sums it over the rows currently rendered:
 *   `slack = Σ max(0, assumed - actual)` and `overlap = Σ max(0, actual - assumed)`,
 *   both in px. A row whose height was measured contributes 0 to both, because its
 *   assumed size *is* its measured height; only estimated (or stale) rows move
 *   these numbers.
 *
 * The elements are found through the same `data-index` attribute the measure
 * batch reads, and a row whose element is missing is skipped rather than counted
 * as a zero-height row. Inside the transcript scroller the chat rows are the only
 * carriers of that attribute.
 *
 * Reads: one `querySelectorAll` plus one `offsetHeight` per rendered row — the
 * first of those may force a layout if the DOM is dirty, the rest are served from
 * it. Callers run this on the HUD's 4 Hz snapshot tick (never per component
 * update), and only while the HUD is enabled.
 */

/** The two members this module reads off a row element. */
export interface RowSlackElementLike {
  getAttribute(name: string): string | null
  /** Row height, the same read the measure batch takes (`element.offsetHeight`). */
  offsetHeight: number
}

/**
 * Structural view of the scroll container. Nodes come back as `unknown` so both
 * the real DOM (`NodeListOf<Element>`) and plain test doubles satisfy it; each
 * node is narrowed to `RowSlackElementLike` before it is read.
 */
export interface RowSlackContainerLike {
  querySelectorAll(selectors: string): ArrayLike<unknown>
}

/** One rendered row, as far as the assumed size needs. */
export interface RowSlackRowLike {
  index: number
  /** Offset the row was placed at — the running sum of the sizes before it. */
  start: number
  /** Offset the next row is placed at: `end - start` is the assumed size. */
  end: number
}

/** Summed deviation of the rendered rows, in px. Both are `>= 0`. */
export interface RowSlackSums {
  /** Σ max(0, assumed - actual): rows placed lower than their real height needs. */
  slack: number
  /** Σ max(0, actual - assumed): rows whose real height overruns their slot. */
  overlap: number
}

/** Shared empty result — never mutated, so callers may return it directly. */
const NO_ROW_SLACK: RowSlackSums = Object.freeze({ slack: 0, overlap: 0 })

/**
 * Narrow one queried node to the two members this module reads. A node that is
 * not an element (or a test double without a height) is skipped, which is what
 * keeps a half-rendered window from throwing on the sampler's tick.
 */
function readRowElement(node: unknown): RowSlackElementLike | null {
  if (typeof node !== 'object' || node === null) return null
  const candidate = node as { getAttribute?: unknown; offsetHeight?: unknown }
  if (typeof candidate.getAttribute !== 'function') return null
  if (typeof candidate.offsetHeight !== 'number') return null
  return candidate as RowSlackElementLike
}

/** `data-index` as a number, or `null` when the attribute is absent/unparsable. */
function readRowIndex(element: RowSlackElementLike): number | null {
  const raw = element.getAttribute(ROW_INDEX_ATTRIBUTE)
  if (raw == null) return null
  const index = Number.parseInt(raw, 10)
  return Number.isNaN(index) ? null : index
}

/**
 * Sum `assumed - actual` over `rows`, reading the live height of each row from
 * `container`. A row with no element in the container is skipped entirely, so a
 * partially rendered window reports the deviation of the rows that do exist
 * rather than of the whole range.
 */
export function measureRenderedRowSlack(
  container: RowSlackContainerLike | null | undefined,
  rows: readonly RowSlackRowLike[] | null | undefined
): RowSlackSums {
  if (!container || !rows || rows.length === 0) return NO_ROW_SLACK

  // One query for the whole rendered window; the per-row reads below are then
  // plain `offsetHeight` lookups. Indexed loop: `ArrayLike` carries no iterator,
  // and this allocates nothing.
  const elements = new Map<number, RowSlackElementLike>()
  const nodes = container.querySelectorAll(`[${ROW_INDEX_ATTRIBUTE}]`)
  for (let i = 0; i < nodes.length; i += 1) {
    const element = readRowElement(nodes[i])
    if (!element) continue
    const index = readRowIndex(element)
    if (index === null) continue
    elements.set(index, element)
  }
  if (elements.size === 0) return NO_ROW_SLACK

  let slack = 0
  let overlap = 0
  for (const row of rows) {
    const element = elements.get(row.index)
    // Not rendered (or not yet in the DOM): nothing to compare against.
    if (!element) continue
    const delta = row.end - row.start - element.offsetHeight
    if (delta > 0) slack += delta
    else overlap -= delta
  }
  return { slack, overlap }
}
