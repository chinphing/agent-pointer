import { nextTick, onMounted, watch, type Ref } from 'vue'
import { t } from '../i18n'

const copyIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`
const checkIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`

function makeCopyButton(onClick: (e: MouseEvent) => void): HTMLButtonElement {
  const btn = document.createElement('button')
  btn.type = 'button'
  btn.className = 'code-copy-btn'
  btn.title = t('common.copyCode')
  btn.innerHTML = copyIconSvg
  btn.addEventListener('click', e => onClick(e as MouseEvent))
  return btn
}

/** Attach copy buttons to markdown code blocks (`.code-block` or bare `<pre>`). */
export function useMarkdownCodeCopy(rootRef: Ref<HTMLElement | null>, getTickSource: () => string) {
  function copyCode(event: MouseEvent) {
    const btn = event.currentTarget as HTMLElement
    const block = btn.closest('.code-block') ?? btn.closest('pre')
    if (!block) return
    const code = block.querySelector('code')
    const text = code?.textContent || block.textContent || ''
    void navigator.clipboard
      .writeText(text)
      .then(() => {
        const prevTitle = btn.title
        btn.innerHTML = checkIconSvg
        btn.title = t('common.copied')
        btn.classList.add('text-success')
        window.setTimeout(() => {
          btn.innerHTML = copyIconSvg
          btn.title = prevTitle
          btn.classList.remove('text-success')
        }, 2000)
      })
      .catch(e => console.error(e))
  }

  function attach() {
    const root = rootRef.value
    if (!root) return

    root.querySelectorAll('.code-block').forEach(block => {
      if (block.querySelector('.code-copy-btn')) return
      block.classList.add('group')
      const lang = block.querySelector('.fence-block-lang')
      const btn = makeCopyButton(copyCode)
      if (lang) lang.appendChild(btn)
      else block.appendChild(btn)
    })

    // Legacy / non-wrapped `<pre>` (e.g. older HTML).
    root.querySelectorAll('pre').forEach(pre => {
      if (pre.closest('.code-block') || pre.querySelector('.code-copy-btn')) return
      pre.classList.add('group')
      pre.appendChild(makeCopyButton(copyCode))
    })
  }

  onMounted(() => {
    nextTick(attach)
  })

  watch(getTickSource, () => {
    nextTick(attach)
  })
}
