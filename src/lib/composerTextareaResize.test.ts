import { describe, expect, it } from 'vitest'
import {
  COMPOSER_TEXTAREA_MAX_HEIGHT_PX as MAX,
  COMPOSER_TEXTAREA_MIN_HEIGHT_PX as MIN,
  createComposerTextareaResizeState,
  resizeComposerTextarea,
  type ComposerTextareaResizeOptions,
  type ComposerTextareaResizeState,
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
  /** How many times the non-destructive (mirror) measurement ran. */
  mirrorCalls = 0

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

  /** Final inline style map — what the element actually carries. */
  inlineStyles(): Record<string, string> {
    return Object.fromEntries(this.inline)
  }
}

/**
 * The pre-refactor algorithm, kept as an oracle: same behaviour contract
 * (grow/shrink/`force`/cap) with none of the write guards.
 *
 * Its fast path is the corrected one — only an empty draft may skip the
 * measure. The original `!value.includes('\n')` shortcut is the bug this file
 * pins: it left a wrapped, newline-free draft stuck at `min`.
 */
function referenceResize(
  el: FakeTextarea,
  previousHeight: number,
  force = false
): number {
  if (!force && el.value === '' && previousHeight === MIN) {
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

/**
 * Mirror model: content height only — independent of the current box. Counts
 * calls so a test can pin whether the pass measured at all.
 */
function mirror(el: FakeTextarea) {
  return (value: string) => {
    el.mirrorCalls += 1
    expect(value).toBe(el.value)
    return el.contentHeight
  }
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

  it('keeps the empty-draft fast path free of layout reads and writes', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state)

    // An empty textarea is one line, so its content height is bounded by the
    // minimum box — the only value that can be proven without measuring.
    expect(el.reads).toEqual({ scrollHeight: 0, clientHeight: 0, offsetHeight: 0 })
    expect(el.writes).toEqual([])
  })

  it('measures a non-empty value even when it has no newline', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('hi', 1)
    resizeComposerTextarea(el, state)

    // A missing newline does not prove a single visual line: an unbroken run
    // soft-wraps. The box has to be read — but a one-line value still fits the
    // minimum box, so nothing is written.
    expect(el.reads).toEqual({ scrollHeight: 1, clientHeight: 1, offsetHeight: 0 })
    expect(el.writes).toEqual([])
    expect(el.usedHeight).toBe(MIN)
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

describe('composer textarea resize — a pasted run with no newline', () => {
  it('grows an empty draft when the pasted run wraps', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })
    expect(el.usedHeight).toBe(MIN)

    const pasted = `https://example.com/${'a'.repeat(120)}`
    el.setContent(pasted, 3)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    expect(el.value).not.toContain('\n')
    expect(el.usedHeight).toBe(PADDING + 3 * LINE)
    expect(el.overflowY).toBe('hidden')
  })

  it('grows the same way with no mirror to measure with', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state, { measureContentHeight: () => null })

    el.setContent('字'.repeat(80), 3)
    resizeComposerTextarea(el, state, { measureContentHeight: () => null })

    expect(el.usedHeight).toBe(PADDING + 3 * LINE)
  })

  it('caps a pasted run at max with a scrollbar', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    el.setContent('x'.repeat(500), 14)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    expect(el.usedHeight).toBe(MAX)
    expect(el.overflowY).toBe('auto')
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
        // `overflow-y` is compared as the observable it exists for — "a box
        // whose content overflows must scroll" — not as a literal string: a
        // pass that changes nothing leaves the class value (`auto`) in place,
        // and `auto` on a box that fits renders exactly like `hidden`.
        if (el.contentHeight > el.usedHeight) expect(el.overflowY).toBe('auto')
        if (el.usedHeight === MAX && el.contentHeight > MAX) {
          // Capped box with overflowing content must scroll.
          expect(el.overflowY).toBe('auto')
        }
      }
    }
  })
})

describe('composer textarea resize — clearing the draft', () => {
  const NO_READS = { scrollHeight: 0, clientHeight: 0, offsetHeight: 0 }

  it('clears a multi-line draft without measuring the box', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb\nc', 3)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(3 * LINE + PADDING)

    const mark = el.writes.length
    el.reads = { ...NO_READS }
    el.setContent('', 1)
    resizeComposerTextarea(el, state)

    // An empty textarea is one line, so the content height is bounded by the
    // minimum box — the shrink probe could only report `min`. It writes `height`
    // *before* it reads `scrollHeight`, so skipping it also skips the
    // write-then-read cycle that dirties the layout.
    expect(el.reads).toEqual(NO_READS)
    expect(el.usedHeight).toBe(MIN)
    expect(el.overflowY).toBe('hidden')
    // The one style an empty draft still needs: the height it occupies.
    expect(el.writesSince(mark)).toEqual([
      { property: 'height', from: `${3 * LINE + PADDING}px`, to: `${MIN}px` }
    ])
  })

  it('clears a capped draft without measuring the box', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent(`a\nb\n${'x'.repeat(500)}`, 14)
    resizeComposerTextarea(el, state)
    expect(el.usedHeight).toBe(MAX)
    expect(el.overflowY).toBe('auto')

    const mark = el.writes.length
    el.reads = { ...NO_READS }
    el.setContent('', 1)
    resizeComposerTextarea(el, state)

    expect(el.reads).toEqual(NO_READS)
    expect(el.usedHeight).toBe(MIN)
    expect(el.overflowY).toBe('hidden')
    const writes = el.writesSince(mark)
    expect(writes.map((write) => write.property).sort()).toEqual(['height', 'overflow-y'])
    for (const write of writes) expect(write.from).not.toBe(write.to)
  })

  it('clears a single-line draft without touching the element', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('hi', 1)
    resizeComposerTextarea(el, state)
    const mark = el.writes.length
    el.reads = { ...NO_READS }

    el.setContent('', 1)
    resizeComposerTextarea(el, state)

    expect(el.reads).toEqual(NO_READS)
    expect(el.writesSince(mark)).toEqual([])
    expect(el.usedHeight).toBe(MIN)
  })

  it('leaves an already-empty draft alone on a repeated resize', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state)
    const mark = el.writes.length
    el.reads = { ...NO_READS }

    resizeComposerTextarea(el, state)

    expect(el.reads).toEqual(NO_READS)
    expect(el.writesSince(mark)).toEqual([])
  })

  it('leaves an already-empty draft alone on a forced pass', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state)
    const mark = el.writes.length
    el.reads = { ...NO_READS }

    resizeComposerTextarea(el, state, { force: true })

    // A forced pass skips the fast paths, but an empty draft still has nothing
    // to measure — the shrink probe (this module's only `scrollHeight` read).
    expect(el.reads.scrollHeight).toBe(0)
    expect(el.writesSince(mark)).toEqual([])
    expect(el.usedHeight).toBe(MIN)
  })

  it('normalises a forced pass when the element rides a taller height', () => {
    const el = new FakeTextarea(2 * LINE + PADDING)
    const state = createComposerTextareaResizeState()

    el.setContent('', 1)
    resizeComposerTextarea(el, state, { force: true })

    expect(el.usedHeight).toBe(MIN)
    expect(el.overflowY).toBe('hidden')
  })
})

/**
 * The hidden mirror (`composerTextareaMirror`) measures the content height
 * without touching the textarea. These cases pin the write counts on the delete
 * path: a delete that leaves the height alone must write nothing, and one that
 * changes it must write only the properties that changed.
 */
describe('composer textarea resize — non-destructive measurement', () => {
  const NO_READS = { scrollHeight: 0, clientHeight: 0, offsetHeight: 0 }

  it('deletes without writing a style when the wrapped height is unchanged', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('aX\nb', 2)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })
    expect(el.usedHeight).toBe(2 * LINE + PADDING)

    const mark = el.writes.length
    el.reads = { ...NO_READS }

    // Remove the `X`: the visual height is unchanged, but the text is not a
    // prefix extension of the previous one, so the old code ran the destructive
    // shrink probe (write `height: min` → read → write the height back).
    el.setContent('a\nb', 2)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    expect(el.writesSince(mark)).toEqual([])
    expect(el.usedHeight).toBe(2 * LINE + PADDING)
    expect(el.mirrorCalls).toBe(1)
    // One `clientHeight` + one `scrollHeight` read — the probe's extra
    // write-then-read cycle never runs.
    expect(el.reads).toEqual({ scrollHeight: 1, clientHeight: 1, offsetHeight: 0 })
  })

  it('writes only the new height when a delete removes a line', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent('a\nb\nc', 3)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })
    expect(el.usedHeight).toBe(3 * LINE + PADDING)

    const mark = el.writes.length
    // Delete `b\n` from the middle: 3 visual lines → 2.
    el.setContent('a\nc', 2)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    expect(el.writesSince(mark)).toEqual([
      {
        property: 'height',
        from: `${3 * LINE + PADDING}px`,
        to: `${2 * LINE + PADDING}px`
      }
    ])
    expect(el.usedHeight).toBe(2 * LINE + PADDING)
    expect(el.overflowY).toBe('hidden')
  })

  it('un-caps with two writes instead of three', () => {
    const el = new FakeTextarea()
    const state = createComposerTextareaResizeState()

    el.setContent(`a\n${'x'.repeat(500)}`, 14)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })
    expect(el.usedHeight).toBe(MAX)
    expect(el.overflowY).toBe('auto')

    const mark = el.writes.length
    el.setContent(`a\n${'x'.repeat(200)}`, 8)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    const writes = el.writesSince(mark)
    expect(writes.map((write) => write.property).sort()).toEqual(['height', 'overflow-y'])
    expect(el.usedHeight).toBe(PADDING + 8 * LINE)
    expect(el.overflowY).toBe('hidden')
  })

  it('applies the measured height when the element mounted taller than the minimum', () => {
    const el = new FakeTextarea(2 * LINE + PADDING)
    const state = createComposerTextareaResizeState()

    el.setContent('hi', 1)
    resizeComposerTextarea(el, state, { measureContentHeight: mirror(el) })

    expect(el.mirrorCalls).toBe(1)
    expect(el.usedHeight).toBe(MIN)
    expect(state.height).toBe(MIN)
  })

  it('falls back to the destructive probe when no measurement is available', () => {
    const mirrored = new FakeTextarea()
    const mirroredState = createComposerTextareaResizeState()
    const probed = new FakeTextarea()
    const probedState = createComposerTextareaResizeState()

    // `null` is what `createComposerTextareaMirror` returns when the mirror
    // cannot be laid out (detached node, no document).
    for (const [el, state, options] of [
      [mirrored, mirroredState, { measureContentHeight: mirror(mirrored) }],
      [probed, probedState, { measureContentHeight: () => null }]
    ] as Array<
      [FakeTextarea, ComposerTextareaResizeState, ComposerTextareaResizeOptions]
    >) {
      el.setContent('aX\nb', 2)
      resizeComposerTextarea(el, state, options)
      el.setContent('a\nb', 2)
      resizeComposerTextarea(el, state, options)
    }

    expect(probed.usedHeight).toBe(mirrored.usedHeight)
    expect(probed.overflowY).toBe(mirrored.overflowY)
    expect(probed.inlineStyles()).toEqual(mirrored.inlineStyles())
    expect(probedState).toEqual(mirroredState)
    // Same end state — but the probe pays two writes for it.
    expect(probed.writes.length).toBeGreaterThan(mirrored.writes.length)
  })
})

describe('composer textarea resize — the mirror path is step-for-step equivalent', () => {
  it('matches the probe path and the oracle over a randomized script, with fewer writes', () => {
    const seed = 0x85ebca6b
    let s = seed
    const rand = () => {
      s = (s + 0x6d2b79f5) | 0
      let t = Math.imul(s ^ (s >>> 15), 1 | s)
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296
    }

    let mirrorWrites = 0
    let probeWrites = 0

    for (const rowsHeight of [MIN, 2 * LINE + PADDING]) {
      const mirrored = new FakeTextarea(rowsHeight)
      const probed = new FakeTextarea(rowsHeight)
      const oracle = new FakeTextarea(rowsHeight)
      const mirroredState = createComposerTextareaResizeState()
      const probedState = createComposerTextareaResizeState()
      let oracleHeight = MIN
      let text = ''

      for (let step = 0; step < 600; step += 1) {
        const roll = rand()
        if (roll < 0.4) text += 'x'
        else if (roll < 0.6) text += '\n'
        else if (roll < 0.8) text = text.slice(0, -1)
        else if (roll < 0.9) text += 'word '.repeat(1 + Math.floor(rand() * 6))
        else if (roll < 0.95) {
          // Edit in the middle: never a prefix extension, so the shrink branch
          // (the one the mirror replaces) is exercised often.
          const at = Math.floor(rand() * (text.length + 1))
          text = `${text.slice(0, at)}${text.slice(at + 1)}`
        } else text = ''

        const visualLines = text
          .split('\n')
          .reduce((total, line) => total + 1 + Math.floor(line.length / 30), 0)
        const force = rand() < 0.04

        for (const el of [mirrored, probed, oracle]) el.setContent(text, visualLines)

        resizeComposerTextarea(mirrored, mirroredState, {
          force,
          measureContentHeight: (value) => (value === mirrored.value ? mirrored.contentHeight : null)
        })
        resizeComposerTextarea(probed, probedState, {
          force,
          measureContentHeight: () => null
        })
        oracleHeight = referenceResize(oracle, oracleHeight, force)

        expect(mirrored.usedHeight).toBe(probed.usedHeight)
        expect(mirrored.usedHeight).toBe(oracle.usedHeight)
        expect(mirrored.overflowY).toBe(probed.overflowY)
        if (mirrored.contentHeight > mirrored.usedHeight) {
          expect(mirrored.overflowY).toBe('auto')
        }
        expect(mirrored.inlineStyles()).toEqual(probed.inlineStyles())
        expect(mirroredState).toEqual(probedState)
      }

      mirrorWrites += mirrored.writes.length
      probeWrites += probed.writes.length
    }

    expect(mirrorWrites).toBeLessThan(probeWrites)
  })
})
