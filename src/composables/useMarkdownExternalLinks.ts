import { nextTick, onBeforeUnmount, watch, type Ref } from 'vue'
import { openExternalUrl } from '../lib/openExternalUrl'

function isExternalWebLink(href: string): boolean {
  const trimmed = href.trim()
  if (!trimmed || trimmed.startsWith('#')) return false
  try {
    const parsed = new URL(trimmed)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

/** Intercept http(s) links in markdown HTML and open via system browser (Tauri) or new tab (Web). */
export function useMarkdownExternalLinks(
  rootRef: Ref<HTMLElement | null>,
  getTickSource: () => string
) {
  function onClick(e: MouseEvent) {
    const target = e.target
    if (!(target instanceof Element)) return
    const anchor = target.closest('a')
    if (!anchor || !rootRef.value?.contains(anchor)) return
    const href = anchor.getAttribute('href')
    if (!href || !isExternalWebLink(href)) return
    e.preventDefault()
    e.stopPropagation()
    void openExternalUrl(href)
  }

  let bound: HTMLElement | null = null

  function bind(el: HTMLElement | null) {
    if (bound === el) return
    if (bound) bound.removeEventListener('click', onClick)
    bound = el
    if (bound) bound.addEventListener('click', onClick)
  }

  watch(rootRef, el => bind(el), { immediate: true })
  watch(getTickSource, () => nextTick(() => bind(rootRef.value)))

  onBeforeUnmount(() => bind(null))
}
