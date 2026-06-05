import { nextTick, onMounted, watch, type Ref } from 'vue'

const copyIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`
const checkIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`

/** Attach copy buttons to `<pre>` inside `rootRef` (uses `.md-body` / `pre` from globals.css). */
export function useMarkdownCodeCopy(rootRef: Ref<HTMLElement | null>, getTickSource: () => string) {
  function copyCode(_index: number, event: MouseEvent) {
    const btn = event.currentTarget as HTMLElement
    const pre = btn.closest('pre')
    if (!pre) return
    const code = pre.querySelector('code')
    const text = code?.textContent || pre.textContent || ''
    void navigator.clipboard.writeText(text).then(() => {
      const prevTitle = btn.title
      btn.innerHTML = checkIconSvg
      btn.title = '已复制'
      btn.classList.add('text-success')
      window.setTimeout(() => {
        btn.innerHTML = copyIconSvg
        btn.title = prevTitle
        btn.classList.remove('text-success')
      }, 2000)
    }).catch(e => console.error(e))
  }

  function attach() {
    const root = rootRef.value
    if (!root) return
    const pres = root.querySelectorAll('pre')
    pres.forEach((pre, index) => {
      if (pre.querySelector('.code-copy-btn')) return
      pre.classList.add('group')
      const btn = document.createElement('button')
      btn.className = 'code-copy-btn'
      btn.title = '复制代码'
      btn.innerHTML = copyIconSvg
      btn.addEventListener('click', e => copyCode(index, e as MouseEvent))
      pre.appendChild(btn)
    })
  }

  onMounted(() => {
    nextTick(attach)
  })

  watch(getTickSource, () => {
    nextTick(attach)
  })
}
