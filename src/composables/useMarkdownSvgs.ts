import { t } from '../i18n'
import { nextTick, onBeforeUnmount, onMounted, watch, type Ref } from 'vue'
import {
  applySvgMountLayout,
  decodeSvgConfigAttr,
  tryParseSvgFence,
} from '../lib/markdownSvg'
import { STREAMING_SVG_STUB } from '../lib/markdownConfig'
import {
  deferUntilInView,
  isInViewForLazyMount,
  type ViewportDeferral,
} from '../lib/markdownChartMount'
import { saveDataUrlAsFile } from '../lib/saveLocalFile'
import { openDiagramZoom, zoomIconSvg } from '../lib/diagramZoom'

const copyIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`
const checkIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`
const downloadIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>`
const codeIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>`

type SvgHostState = {
  boundConfig: string
  viewportGate: ViewportDeferral | null
}

/** Cleaned SVG markup cache — survives v-html host recreation while trailing text streams. */
const cleanedSvgByConfig = new Map<string, string>()
const CLEANED_SVG_CACHE_MAX = 32

function cacheCleanedSvg(encoded: string, svg: string) {
  if (cleanedSvgByConfig.has(encoded)) {
    cleanedSvgByConfig.delete(encoded)
  }
  cleanedSvgByConfig.set(encoded, svg)
  while (cleanedSvgByConfig.size > CLEANED_SVG_CACHE_MAX) {
    const oldest = cleanedSvgByConfig.keys().next().value
    if (oldest == null) break
    cleanedSvgByConfig.delete(oldest)
  }
}

function isStreamingSvgStub(raw: string): boolean {
  const t = raw.trim()
  return t === STREAMING_SVG_STUB || t.includes('data-pointer-svg-pending')
}

export type MarkdownSvgsOptions = {
  /** When true, never mount SVG — show a pending placeholder only. */
  isStreaming?: () => boolean
}

/** Mount sanitized SVG diagrams for `.md-svg` hosts inside markdown HTML. */
export function useMarkdownSvgs(
  rootRef: Ref<HTMLElement | null>,
  getTickSource: () => string,
  options: MarkdownSvgsOptions = {}
) {
  const hosts = new Map<HTMLElement, SvgHostState>()

  function flashButton(btn: HTMLButtonElement, okTitle: string) {
    const prev = btn.innerHTML
    const prevTitle = btn.title
    btn.innerHTML = checkIconSvg
    btn.title = okTitle
    btn.classList.add('text-success')
    window.setTimeout(() => {
      btn.innerHTML = prev
      btn.title = prevTitle
      btn.classList.remove('text-success')
    }, 1600)
  }

  function ensureToolbar(host: HTMLElement) {
    const toolbar = host.querySelector('.md-svg-toolbar')
    if (!(toolbar instanceof HTMLElement) || toolbar.dataset.ready === '1') return
    toolbar.dataset.ready = '1'

    const copyBtn = document.createElement('button')
    copyBtn.type = 'button'
    copyBtn.className = 'md-svg-btn'
    copyBtn.title = t('common.copySource')
    copyBtn.setAttribute('aria-label', t('common.copySource'))
    copyBtn.innerHTML = copyIconSvg
    copyBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const encoded = host.getAttribute('data-svg-config')
      const raw = encoded ? decodeSvgConfigAttr(encoded) : null
      if (!raw) {
        console.warn('[markdownSvgs] copy: missing config')
        return
      }
      void navigator.clipboard.writeText(raw).then(() => flashButton(copyBtn, t('common.copied'))).catch(err => {
        console.error('[markdownSvgs] copy failed', err)
      })
    })

    const downloadBtn = document.createElement('button')
    downloadBtn.type = 'button'
    downloadBtn.className = 'md-svg-btn'
    downloadBtn.title = t('markdown.exportSvg')
    downloadBtn.setAttribute('aria-label', t('markdown.exportSvg'))
    downloadBtn.innerHTML = downloadIconSvg
    downloadBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const encoded = host.getAttribute('data-svg-config')
      const raw = encoded ? decodeSvgConfigAttr(encoded) : null
      if (!raw) {
        console.warn('[markdownSvgs] export: missing config')
        return
      }
      const parsed = tryParseSvgFence(raw)
      if (!parsed.ok) {
        console.warn('[markdownSvgs] export: invalid svg', parsed.reason)
        return
      }
      const dataUrl =
        'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(parsed.svg)
      const fileName = `pointer-diagram-${Date.now()}.svg`
      void saveDataUrlAsFile(dataUrl, fileName, [{ name: 'SVG', extensions: ['svg'] }])
        .then(result => {
          if (result === 'saved') flashButton(downloadBtn, t('markdown.exported'))
        })
        .catch(err => {
          console.error('[markdownSvgs] export failed', err)
        })
    })

    const sourceBtn = document.createElement('button')
    sourceBtn.type = 'button'
    sourceBtn.className = 'md-svg-btn'
    sourceBtn.title = t('markdown.viewSource')
    sourceBtn.setAttribute('aria-label', t('markdown.viewSource'))
    sourceBtn.innerHTML = codeIconSvg
    sourceBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const pre = host.querySelector('.md-svg-source')
      if (!(pre instanceof HTMLElement)) return
      const open = pre.hasAttribute('hidden')
      if (open) {
        const encoded = host.getAttribute('data-svg-config')
        const raw = encoded ? decodeSvgConfigAttr(encoded) : ''
        pre.textContent = raw || ''
        pre.removeAttribute('hidden')
        sourceBtn.title = t('markdown.hideSource')
        sourceBtn.setAttribute('aria-label', t('markdown.hideSource'))
        sourceBtn.classList.add('md-svg-btn-active')
      } else {
        pre.setAttribute('hidden', '')
        sourceBtn.title = t('markdown.viewSource')
        sourceBtn.setAttribute('aria-label', t('markdown.viewSource'))
        sourceBtn.classList.remove('md-svg-btn-active')
      }
    })

    const zoomBtn = document.createElement('button')
    zoomBtn.type = 'button'
    zoomBtn.className = 'md-svg-btn'
    zoomBtn.title = t('markdown.zoom')
    zoomBtn.setAttribute('aria-label', t('markdown.zoom'))
    zoomBtn.innerHTML = zoomIconSvg
    zoomBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const frame = host.querySelector('.md-svg-frame')
      const svg = frame?.querySelector('svg')
      if (svg instanceof SVGElement) openDiagramZoom(svg)
    })

    toolbar.append(zoomBtn, copyBtn, downloadBtn, sourceBtn)
  }

  function setToolbarVisible(host: HTMLElement, visible: boolean) {
    const toolbar = host.querySelector('.md-svg-toolbar')
    if (!(toolbar instanceof HTMLElement)) return
    toolbar.hidden = !visible
  }

  function showStatus(host: HTMLElement, message: string, kind: 'pending' | 'error') {
    const frame = host.querySelector('.md-svg-frame')
    hosts.delete(host)
    host.classList.toggle('md-svg--invalid', kind === 'error')
    host.classList.toggle('md-svg--pending', kind === 'pending')
    setToolbarVisible(host, kind !== 'pending')
    if (!(frame instanceof HTMLElement)) return
    frame.replaceChildren()
    const status = document.createElement('div')
    status.className = `md-svg-status md-svg-status-${kind}`
    status.textContent = message
    frame.appendChild(status)
  }

  function mountOrUpdate(host: HTMLElement) {
    ensureToolbar(host)

    const encoded = host.getAttribute('data-svg-config')
    if (!encoded) {
      showStatus(host, t('markdown.diagramMissingConfig'), 'error')
      return
    }
    const raw = decodeSvgConfigAttr(encoded)
    if (raw == null) {
      showStatus(host, t('markdown.diagramUnparseable'), 'error')
      return
    }

    // Pending only for incomplete fences (stub), not for the whole assistant turn.
    if (isStreamingSvgStub(raw)) {
      showStatus(host, t('markdown.diagramGenerating'), 'pending')
      return
    }

    const prevEarly = hosts.get(host)
    if (prevEarly?.boundConfig === encoded && host.querySelector('.md-svg-frame > svg')) {
      setToolbarVisible(host, true)
      return
    }
    const inView = isInViewForLazyMount(host)
    if (prevEarly?.boundConfig === encoded && prevEarly.viewportGate && !inView) {
      return
    }
    if (!inView) {
      prevEarly?.viewportGate?.disconnect()
      const viewportGate = deferUntilInView(host, () => {
        const state = hosts.get(host)
        if (state) state.viewportGate = null
        if (import.meta.env.DEV) {
          console.info('[markdownSvgs] diagram entered view; mount')
        }
        mountOrUpdate(host)
      })
      hosts.set(host, { boundConfig: encoded, viewportGate })
      return
    }
    prevEarly?.viewportGate?.disconnect()

    let cleaned = cleanedSvgByConfig.get(encoded)
    if (!cleaned) {
      const parsed = tryParseSvgFence(raw)
      if (!parsed.ok) {
        // Closed fences that fail XML parse are invalid, not "still generating".
        const asPending =
          parsed.reason === 'empty' ||
          (parsed.reason === 'not_svg' && !raw.includes('</svg>'))
        if (!asPending) {
          console.warn('[markdownSvgs] svg not mountable', parsed.reason)
        }
        showStatus(
          host,
          asPending ? t('markdown.diagramGenerating') : t('markdown.diagramInvalid'),
          asPending ? 'pending' : 'error'
        )
        return
      }
      cleaned = parsed.svg
      cacheCleanedSvg(encoded, cleaned)
    }

    const prev = hosts.get(host)
    if (prev?.boundConfig === encoded && host.querySelector('.md-svg-frame > svg')) {
      setToolbarVisible(host, true)
      return
    }

    host.classList.remove('md-svg--invalid', 'md-svg--pending')
    setToolbarVisible(host, true)
    let frame = host.querySelector('.md-svg-frame')
    if (!(frame instanceof HTMLElement)) {
      frame = document.createElement('div')
      frame.className = 'md-svg-frame'
      host.appendChild(frame)
    }
    frame.replaceChildren()

    try {
      if (typeof DOMParser === 'undefined') {
        frame.textContent = 'SVG'
        console.warn('[markdownSvgs] DOMParser unavailable; skip mount')
        return
      }
      const doc = new DOMParser().parseFromString(cleaned, 'image/svg+xml')
      const parseError = doc.querySelector('parsererror')
      if (parseError) {
        showStatus(host, t('markdown.diagramInvalid'), 'error')
        return
      }
      const root = doc.documentElement
      if (!root || root.localName.toLowerCase() !== 'svg') {
        showStatus(host, t('markdown.diagramInvalid'), 'error')
        return
      }
      const imported = document.importNode(root, true)
      if (!(imported instanceof SVGElement)) {
        showStatus(host, t('markdown.diagramInvalid'), 'error')
        return
      }
      imported.setAttribute('role', 'img')
      if (!imported.getAttribute('aria-label')) {
        imported.setAttribute('aria-label', 'diagram')
      }
      frame.appendChild(imported)
      applySvgMountLayout(imported)
      imported.addEventListener('click', e => {
        e.preventDefault()
        e.stopPropagation()
        openDiagramZoom(imported)
      })
      hosts.set(host, { boundConfig: encoded, viewportGate: null })
      host.dataset.svgBound = encoded
      console.info('[markdownSvgs] mounted svg host')
    } catch (err) {
      console.error('[markdownSvgs] mount failed', err)
      showStatus(host, t('markdown.diagramRenderFailed'), 'error')
    }
  }

  function sync() {
    const root = rootRef.value
    if (!root) return
    const found = root.querySelectorAll('.md-svg')
    const alive = new Set<HTMLElement>()
    for (const node of Array.from(found)) {
      if (!(node instanceof HTMLElement)) continue
      alive.add(node)
      mountOrUpdate(node)
    }
    for (const host of Array.from(hosts.keys())) {
      if (!alive.has(host)) {
        try {
          hosts.get(host)?.viewportGate?.disconnect()
        } catch (err) {
          console.warn('[markdownSvgs] viewportGate disconnect failed', err)
        }
        hosts.delete(host)
      }
    }
  }

  onMounted(() => {
    void nextTick(sync)
  })
  onBeforeUnmount(() => {
    for (const state of hosts.values()) {
      try {
        state.viewportGate?.disconnect()
      } catch (err) {
        console.warn('[markdownSvgs] viewportGate disconnect failed', err)
      }
    }
    hosts.clear()
  })
  watch(
    () => getTickSource(),
    () => {
      void nextTick(sync)
    }
  )
  watch(
    () => options.isStreaming?.() === true,
    () => {
      void nextTick(sync)
    }
  )
}
