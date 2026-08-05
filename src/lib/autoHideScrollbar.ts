/** Show thin scrollbar while scrolling; hide again after idle (sidebar / chat). */

const hideTimers = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>()

const HIDE_DELAY_MS = 600

/** Toggle `.is-scrolling` on the scroll target for `.auto-hide-scrollbar` CSS. */
export function showScrollbarWhileScrolling(event: Event): void {
  const target = event.currentTarget
  if (!(target instanceof HTMLElement)) return
  target.classList.add('is-scrolling')
  const existing = hideTimers.get(target)
  if (existing) clearTimeout(existing)
  hideTimers.set(
    target,
    setTimeout(() => {
      target.classList.remove('is-scrolling')
      hideTimers.delete(target)
    }, HIDE_DELAY_MS)
  )
}
