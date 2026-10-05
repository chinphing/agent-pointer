import { describe, expect, it } from 'vitest'
import { messageRowMemoDeps, type MessageRowMemoInput } from './messageListRowMemo'

/**
 * The row-level `v-memo` checklist: every input that must still be able to make a
 * virtual row repaint. A missing one is a silent visual freeze, so each field gets
 * its own assertion — flipping it has to change the dependency vector (i.e. force a
 * memo miss), and leaving it alone has to keep the memo hit.
 */
const baseline: MessageRowMemoInput = {
  start: 0,
  index: 0,
  turn: { id: 't1' },
  expanded: true,
  showElapsed: false,
  elapsedLabel: '',
  files: null,
  changeSummaryExpanded: false,
  spawnPlan: null,
  searchMatchIds: [],
  activeSearchMessageId: null,
  activeSearchToolCallId: null,
  focusHighlightMessageId: null,
  activeBoard: null,
  activeBoardSticky: false,
  conversationId: 'c1',
  taskBoards: {},
  glueEnabled: false
}

/** A different value per field — the "input moved" half of the checklist. */
const moved: Record<keyof MessageRowMemoInput, unknown> = {
  // geometry
  start: 28,
  index: 4,
  // turn identity (layout rebuild: entries / collapsedEntries / state / hiddenCount)
  turn: { id: 't2' },
  // collapsed vs expanded projection
  expanded: false,
  // 「工作」chip visibility + its 1 Hz label
  showElapsed: true,
  elapsedLabel: '12s',
  // ChangeSummary
  files: [{ path: 'a.ts' }],
  changeSummaryExpanded: true,
  // sub-agent frame placement
  spawnPlan: { tree: {} },
  // search / focus highlight
  searchMatchIds: ['m1'],
  activeSearchMessageId: 'm1',
  activeSearchToolCallId: 'tc1',
  focusHighlightMessageId: 'm1',
  // task board chrome
  activeBoard: { storeKey: 'b1' },
  activeBoardSticky: true,
  conversationId: 'c2',
  taskBoards: { b1: {} },
  // glue visibility
  glueEnabled: true
}

const FIELDS = Object.keys(baseline) as Array<keyof MessageRowMemoInput>

describe('messageRowMemoDeps', () => {
  it('pins the input list — a new row input must be added here and to the vector', () => {
    expect(FIELDS.slice().sort()).toEqual([
      'activeBoard',
      'activeBoardSticky',
      'activeSearchMessageId',
      'activeSearchToolCallId',
      'changeSummaryExpanded',
      'conversationId',
      'elapsedLabel',
      'expanded',
      'files',
      'focusHighlightMessageId',
      'glueEnabled',
      'index',
      'searchMatchIds',
      'showElapsed',
      'spawnPlan',
      'start',
      'taskBoards',
      'turn'
    ])
    // Every declared input reaches the vector (no field silently dropped).
    expect(messageRowMemoDeps(baseline)).toHaveLength(FIELDS.length)
  })

  it('covers every field in `moved` (no field without a value to move it to)', () => {
    expect(Object.keys(moved).slice().sort()).toEqual(FIELDS.slice().sort())
  })

  it.each(FIELDS)('changing `%s` forces a memo miss', field => {
    const before = messageRowMemoDeps(baseline)
    const after = messageRowMemoDeps({ ...baseline, [field]: moved[field] })
    const changed = after.some((value, index) => !Object.is(value, before[index]))
    expect(changed, `\`${field}\` must be part of the row memo vector`).toBe(true)
  })

  it('keeps the memo hit when nothing moved (same values, fresh object)', () => {
    const before = messageRowMemoDeps(baseline)
    const after = messageRowMemoDeps({ ...baseline })
    expect(after).toHaveLength(before.length)
    expect(after.every((value, index) => Object.is(value, before[index]))).toBe(true)
  })

  it('keeps the memo hit when only an unrelated value changes', () => {
    // Two rows whose geometry differs must still be able to memo independently:
    // the vector is compared per row, so a second row's inputs are irrelevant.
    const rowA = messageRowMemoDeps({ ...baseline, start: 0, index: 0 })
    const rowB = messageRowMemoDeps({ ...baseline, start: 300, index: 1 })
    const rowAAgain = messageRowMemoDeps({ ...baseline, start: 0, index: 0 })
    expect(rowAAgain.every((value, index) => Object.is(value, rowA[index]))).toBe(true)
    expect(rowB.some((value, index) => !Object.is(value, rowA[index]))).toBe(true)
  })
})
