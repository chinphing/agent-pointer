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
 * a property is only written when its value actually changes, and the
 * destructive shrink probe only runs when the content may have shrunk.
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
}

export function resizeComposerTextarea(
  el: ComposerTextareaResizeTarget,
  state: ComposerTextareaResizeState,
  options: ComposerTextareaResizeOptions = {}
): ComposerTextareaResizeState {
  const min = options.min ?? COMPOSER_TEXTAREA_MIN_HEIGHT_PX
  const max = options.max ?? COMPOSER_TEXTAREA_MAX_HEIGHT_PX
  const force = options.force === true

  const value = el.value

  // Fast path 1: single-line text that never left the min height needs neither
  // a layout read nor a write. Reading scrollHeight/offsetHeight while the
  // streaming list is mid-measure forces a synchronous reflow in the keystroke
  // frame and flushes pending virtualizer row measurements.
  if (!force && !value.includes('\n') && state.height === min) return state

  // Fast path 2: the text is untouched since the last evaluation (composition
  // timers, repeated `nextTick` calls). Nothing can have changed.
  if (!force && state.measuredValue === value) return state

  // Fast path 3: an empty draft. An empty textarea is one line, so its content
  // height is bounded by the minimum box — the shrink probe below could only
  // ever report `min`, whatever the box happens to be. Skip the whole measure
  // pass: the probe writes `height` *before* it reads `scrollHeight`, and that
  // write is what dirties the layout in the very frame the draft flush patches
  // the DOM.
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
    // clamps to `clientHeight`. Shrink the box to the minimum to read the real
    // content height — one write, one read — then write back only what changed.
    writeComposerTextareaOverflowY(el, state, 'hidden')
    // `box > min` here, so this is always a real change; bypass the state guard
    // (it can be stale when the element mounted at its rows height).
    el.style.setProperty('height', `${min}px`)
    state.height = min
    const measured = el.scrollHeight
    nextHeight = Math.min(Math.max(measured, min), max)
    overflowY = measured > max ? 'auto' : 'hidden'
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
