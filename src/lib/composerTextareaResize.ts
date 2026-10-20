/**
 * Composer textarea auto-grow — the fallback used when the engine does not
 * support `field-sizing: content` (see `.composer-textarea` in globals.css).
 *
 * Lives outside the component so the write/measure decisions are unit-testable
 * without mounting Composer.
 *
 * Rewriting an identical inline style on every keystroke marks the textarea
 * style-dirty and forces a layout pass, which can flush pending virtualizer row
 * measurements mid-stream → visible list jump. So every write here is guarded:
 * a property is only written when its value actually changes.
 *
 * "How tall is the content?" is answered by a non-destructive measurement
 * (`measureContentHeight`, see `composerTextareaMirror`) whenever the caller
 * provides one, so a delete that leaves the height unchanged writes nothing at
 * all. The old destructive probe (shrink the box to `min`, read `scrollHeight`,
 * write back) remains only as the fallback for callers with no mirror.
 */

export const COMPOSER_TEXTAREA_MIN_HEIGHT_PX = 36
export const COMPOSER_TEXTAREA_MAX_HEIGHT_PX = 250

/** The slice of the textarea element the resize logic touches. */
export interface ComposerTextareaResizeTarget {
  readonly value: string
  readonly scrollHeight: number
  readonly clientHeight: number
  readonly style: {
    setProperty(property: string, value: string): void
  }
}

/**
 * Inline style values this module owns on the textarea.
 *
 * `null` means "not written by us yet" — the element still carries whatever the
 * CSS class provides, so the first write is always a real change.
 */
export interface ComposerTextareaResizeState {
  /** Inline height in px (starts at the min, matching the CSS min-height). */
  height: number
  minHeight: number | null
  overflowY: 'auto' | 'hidden' | null
  /** `value` seen by the last full evaluation; `null` = never evaluated. */
  measuredValue: string | null
}

export function createComposerTextareaResizeState(): ComposerTextareaResizeState {
  return {
    height: COMPOSER_TEXTAREA_MIN_HEIGHT_PX,
    minHeight: null,
    overflowY: null,
    measuredValue: null
  }
}

export interface ComposerTextareaResizeOptions {
  /** Width changed (sidebar collapse / window resize) → soft wraps may differ. */
  force?: boolean
  min?: number
  max?: number
  /**
   * Non-destructive content-height measurement (see `composerTextareaMirror`).
   *
   * Returns the height the text would occupy at the textarea's current inline
   * size, or `null` when no such measurement is available — then the destructive
   * shrink probe runs instead. Providing it removes the two style writes that
   * probe owes on every pass that might have shrunk.
   */
  measureContentHeight?: (value: string) => number | null
}

export function resizeComposerTextarea(
  el: ComposerTextareaResizeTarget,
  state: ComposerTextareaResizeState,
  options: ComposerTextareaResizeOptions = {}
): ComposerTextareaResizeState {
  const min = options.min ?? COMPOSER_TEXTAREA_MIN_HEIGHT_PX
  const max = options.max ?? COMPOSER_TEXTAREA_MAX_HEIGHT_PX
  const force = options.force === true
  const measureContentHeight = options.measureContentHeight

  const value = el.value

  // Fast path 1: an empty draft that never left the min height needs neither a
  // layout read nor a write. An empty textarea is one line, so its content
  // height is bounded by the minimum box — that is the one case where "how
  // tall?" is answerable without measuring. The read is the textarea's own
  // scrollHeight (it lives in the composer frame), not the transcript's.
  //
  // A newline-free value is *not* enough to conclude "one line": a pasted URL
  // or a run of CJK wraps at the padding edge without any newline, so the
  // single-line assumption silently kept such a draft at `min` and the text was
  // clipped from the next line down. Only the empty string is provably one
  // line; every other value is measured below.
  if (!force && value === '' && state.height === min) return state

  // Fast path 2: the text is untouched since the last evaluation (composition
  // timers, repeated `nextTick` calls). Nothing can have changed.
  if (!force && state.measuredValue === value) return state

  // Fast path 3: an empty draft that still needs normalising (a forced pass, or
  // a box this module left taller than `min`). Its content height is bounded by
  // the minimum box, so the shrink probe below could only ever report `min`,
  // whatever the box happens to be. Skip the whole measure pass: the probe
  // writes `height` *before* it reads `scrollHeight`, and that write is what
  // dirties the layout in the very frame the draft flush patches the DOM.
  if (value === '') {
    // `state.height !== min` means this module wrote a taller height, so the
    // element is genuinely taller than an empty draft needs. A forced pass
    // (width change) still has to look, because the element can be riding a
    // taller CSS `rows` height this module never wrote.
    if (state.height !== min || (force && el.clientHeight > min)) {
      writeComposerTextareaMinHeight(el, state, min)
      // Bypass the height guard: it can be stale when the element mounted at
      // its rows height, and this branch only runs when the box is taller than
      // an empty draft needs — so writing `min` is always a real change.
      el.style.setProperty('height', `${min}px`)
      state.height = min
      writeComposerTextareaOverflowY(el, state, 'hidden')
    }
    state.measuredValue = value
    return state
  }

  // Both reads below share a single layout flush.
  const box = el.clientHeight
  const probe = el.scrollHeight

  // `scrollHeight` is the true content height whenever the content overflows
  // the box, so no destructive measure is needed when it can only have grown:
  // either it already overflows, or the text is a strict extension of the text
  // seen at the last evaluation (appending characters can never make the
  // wrapped content shorter).
  const canOnlyGrow =
    probe > box ||
    (!force &&
      state.measuredValue !== null &&
      box === state.height &&
      value.startsWith(state.measuredValue))

  let nextHeight: number
  let overflowY: 'auto' | 'hidden'

  if (canOnlyGrow) {
    nextHeight = Math.min(Math.max(probe, min), max)
    overflowY = probe > max ? 'auto' : 'hidden'
  } else if (box <= min) {
    // The content still fits the minimum box (the CSS min-height guarantees
    // box >= min): the height is already right, so leave the element alone.
    state.height = min
    state.measuredValue = value
    return state
  } else {
    // The content may have shrunk below the current box, and `scrollHeight`
    // clamps to `clientHeight`, so it cannot answer "how tall is the content?".
    // Ask the non-destructive measurement first: it leaves this element alone,
    // so a delete that keeps the height ends the pass with zero writes.
    const measured = measureContentHeight ? measureContentHeight(value) : null
    if (measured != null && Number.isFinite(measured)) {
      nextHeight = Math.min(Math.max(measured, min), max)
      overflowY = measured > max ? 'auto' : 'hidden'
      // The guarded write below compares against the tracked height, and that
      // record can be stale: an element that mounted taller than anything this
      // module wrote (same hazard the empty-draft branch above bypasses).
      // Nothing has been written this pass, so `box` is the element's real
      // height — resync, or a measured shrink would be swallowed and the box
      // would never come back down.
      if (box !== state.height) state.height = box
    } else {
      // Fallback (no mirror available): the destructive probe. Shrink the box to
      // the minimum to read the real content height — one write, one read — then
      // write back only what changed. Each write dirties layout, so this only
      // runs when the caller cannot measure without touching the element.
      writeComposerTextareaOverflowY(el, state, 'hidden')
      // `box > min` here, so this is always a real change; bypass the state guard
      // (it can be stale when the element mounted at its rows height).
      el.style.setProperty('height', `${min}px`)
      state.height = min
      const probe = el.scrollHeight
      nextHeight = Math.min(Math.max(probe, min), max)
      overflowY = probe > max ? 'auto' : 'hidden'
    }
  }

  writeComposerTextareaMinHeight(el, state, min)
  writeComposerTextareaHeight(el, state, nextHeight)
  writeComposerTextareaOverflowY(el, state, overflowY)
  state.measuredValue = value
  return state
}

function writeComposerTextareaHeight(
  el: ComposerTextareaResizeTarget,
  state: ComposerTextareaResizeState,
  height: number
) {
  if (state.height === height) return
  el.style.setProperty('height', `${height}px`)
  state.height = height
}

function writeComposerTextareaMinHeight(
  el: ComposerTextareaResizeTarget,
  state: ComposerTextareaResizeState,
  min: number
) {
  if (state.minHeight === min) return
  el.style.setProperty('min-height', `${min}px`)
  state.minHeight = min
}

function writeComposerTextareaOverflowY(
  el: ComposerTextareaResizeTarget,
  state: ComposerTextareaResizeState,
  overflowY: 'auto' | 'hidden'
) {
  if (state.overflowY === overflowY) return
  el.style.setProperty('overflow-y', overflowY)
  state.overflowY = overflowY
}
