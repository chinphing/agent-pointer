import { describe, expect, it } from 'vitest'
import {
  COMPOSER_TEXTAREA_MAX_HEIGHT_PX as MAX,
  COMPOSER_TEXTAREA_MIN_HEIGHT_PX as MIN,
  createComposerTextareaResizeState,
  resizeComposerTextarea,
  type ComposerTextareaResizeTarget
} from './composerTextareaResize'

const LINE = 20
const PADDING = 16

/**
 * Minimal textarea model: a real engine clamps `scrollHeight` to
 * `clientHeight`, which is exactly why the shrink path has to shrink the box
 * before it can read the content height.
 */
class FakeTextarea implements ComposerTextareaResizeTarget {
  value = ''
  /** Ground truth: the height the content would occupy. */
  contentHeight = MIN
  reads = { scrollHeight: 0, clientHeight: 0, offsetHeight: 0 }
  writes: Array<{ property: string; from: string; to: string }> = []

  private inline = new Map<string, string>()

  readonly style = {
    setProperty: (property: string, value: string) => {
      const from = this.inline.get(property) ?? ''
      this.writes.push({ property, from, to: value })
      this.inline.set(property, value)
    }
  }

  constructor(private rowsHeight = MIN) {}

  get usedHeight() {
    const px = (name: string, fallback: number) => {
      const raw = this.inline.get(name)
      if (raw == null || raw === '') return fallback
      const n = Number.parseFloat(raw)
      return Number.isFinite(n) ? n : fallback
    }
    const declared = px('height', this.rowsHeight)
    const lower = Math.max(MIN, px('min-height', MIN))
    const upper = Math.min(MAX, px('max-height', MAX))
    return Math.min(Math.max(declared, lower), upper)
  }

  get clientHeight() {
    this.reads.clientHeight++
    return this.usedHeight
  }

  get scrollHeight() {
    this.reads.scrollHeight++
    return Math.max(this.contentHeight, this.usedHeight)
  }

  get offsetHeight() {
    this.reads.offsetHeight++
    return this.usedHeight
  }

  /** Inline overflow-y, falling back to the `overflow-y-auto` class. */
  get overflowY() {
    return this.inline.get('overflow-y') ?? 'auto'
  }

  setContent(value: string, visualLines: number) {
    this.value = value
    this.contentHeight = PADDING + visualLines * LINE
  }

  writesSince(mark: number) {
    return this.writes.slice(mark)
  }
}

/**
 * The pre-refactor algorithm, kept as an oracle: same behaviour contract
 * (grow/shrink/`force`/cap) with none of the write guards.
 */
function referenceResize(
  el: FakeTextarea,
  previousHeight: number,
  force = false
): number {
  if (!force && !el.value.includes('\n') && previousHeight === MIN) {
    return previousHeight
  }
  el.style.setProperty('overflow-y', 'hidden')
  el.style.setProperty('min-height', '0')
  el.style.setProperty('height', '0')
  void el.offsetHeight
  const content = el.scrollHeight
  const nextHeight = Math.min(Math.max(content, MIN), MAX)
  el.style.setProperty('min-height', `${MIN}px`)
  el.style.setProperty('height', `${nextHeight}px`)
  el.style.setProperty('overflow-y', content > MAX ? 'auto' : 'hidden')
  return nextHeight
}

describe('composer textarea resize — style writes', () => {
  it('writes nothing when a keystroke leaves the height unchanged', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(2 * LINE + PADDING)

    const mark = el.writes.length
    // One more character on the same line: same visual height.
    el.setContent('a\nbc', 2)
    resizeComposerTextarea(el, state)

    expect(el.writesSince(mark)).toEqual([])
  })

  it('never writes a style value that is already there', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state)
    el.setContent('a\nbc', 2)
    resizeComposerTextarea(el, state)
    el.setContent('a\nbcd', 2)
    resizeComposerTextarea(el, state)
    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state)

    for (const write of el.writes) {
      expect(write.from).not.toBe(write.to)
    }
  })

  it('shrinks back to one line after a multi-line draft, with a single write', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state)
    const mark = el.writes.length

    el.setContent('ab', 1)
    resizeComposerTextarea(el, state)

    expect(el.usedHeight).toBe(MIN)
    // Only the shrink probe writes; the measured height equals the probe height,
    // so the restore is skipped.
    expect(el.writesSince(mark)).toHaveLength(1)
  })

  it('keeps the single-line fast path free of layout reads and writes', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('hi', 1)
    resizeComposerTextarea(el, state)

    expect(el.reads).toEqual({ scrollHeight: 0, clientHeight: 0, offsetHeight: 0 })
    expect(el.writes).toEqual([])
  })

  it('grows on a newline and caps at max with a scrollbar', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(2 * LINE + PADDING)
    expect(el.overflowY).toBe('hidden')

    el.setContent('x'.repeat(500), 14)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(MAX)
    expect(el.overflowY).toBe('auto')

    el.setContent('x'.repeat(20), 2)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(2 * LINE + PADDING)
    expect(el.overflowY).toBe('hidden')
  })

  it('re-measures on a width change even when the text is untouched', () => {
    const el = new FakeTextarea(2 * LINE + PADDING)
    const state = createComposerTextareaResizeState()

    el.setContent('abc', 1)
    resizeComposerTextarea(el, state, { force: true })

    expect(el.usedHeight).toBe(MIN)
  })

  it('leaves the element alone when a width change cannot alter the height', () => {
    const el = new FakeTextarea(MIN)
    const state = createComposerTextareaResizeState()

    el.setContent('abc', 1)
    resizeComposerTextarea(el, state)
    const mark = el.writes.length

    resizeComposerTextarea(el, state, { force: true })

    expect(el.writesSince(mark)).toEqual([])
    expect(el.usedHeight).toBe(MIN)
  })
})

describe('composer textarea resize — matches the previous behaviour', () => {
  it('agrees with the oracle over a randomized typing script', () => {
    const seed = 0x9e3779b9
    let s = seed
    const rand = () => {
      s = (s + 0x6d2b79f5) | 0
      let t = Math.imul(s ^ (s >>> 15), 1 | s)
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296
    }

    for (const rowsHeight of [MIN, 2 * LINE + PADDING]) {
      const el = new FakeTextarea(rowsHeight)
      const oracle = new FakeTextarea(rowsHeight)
      const state = createComposerTextareaResizeState()
      let oracleHeight = MIN
      let text = ''

      for (let step = 0; step < 400; step += 1) {
        const roll = rand()
        if (roll < 0.45) text += 'x'
        else if (roll < 0.7) text += '\n'
        else if (roll < 0.85) text = text.slice(0, -1)
        else if (roll < 0.95) text += 'word '.repeat(1 + Math.floor(rand() * 6))
        else text = ''

        const explicitLines = text.split('\n')
        const visualLines = explicitLines.reduce(
          (total, line) => total + 1 + Math.floor(line.length / 30),
          0
        )
        const force = rand() < 0.03

        el.setContent(text, visualLines)
        resizeComposerTextarea(el, state, { force })
        oracle.setContent(text, visualLines)
        oracleHeight = referenceResize(oracle, oracleHeight, force)

        expect(el.usedHeight).toBe(oracle.usedHeight)
        expect(el.overflowY).toBe(oracle.overflowY)
        if (el.usedHeight === MAX && el.contentHeight > MAX) {
          // Capped box with overflowing content must scroll.
          expect(el.overflowY).toBe('auto')
        }
      }
    }
  })
})
