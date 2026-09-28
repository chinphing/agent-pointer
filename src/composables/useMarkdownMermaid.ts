import { nextTick, onBeforeUnmount, onMounted, watch, type Ref } from 'vue'
import { t } from '../i18n'
import {
  applySvgMountLayout,
  decodeSvgConfigAttr,
  sanitizeSvgMarkup,
} from '../lib/markdownSvg'
import {
  isStreamingMermaidStub,
  isSupportedMermaidSource,
  mermaidInitializeConfig,
  mermaidThemeCacheKey,
  mermaidThemeScheme,
  roundMermaidSvgRects,
  prepareMermaidSource,
} from '../lib/markdownMermaid'
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

type MermaidModule = typeof import('mermaid').default
let mermaidModulePromise: Promise<MermaidModule> | null = null
let appliedThemeScheme: 'light' | 'dark' | null = null
let renderSeq = 0

function applyHostMermaidTheme(api: MermaidModule): boolean {
  const scheme = mermaidThemeScheme()
  if (appliedThemeScheme === scheme) return false
  api.initialize(mermaidInitializeConfig())
  appliedThemeScheme = scheme
  cleanedSvgByConfig.clear()
  console.info('[markdownMermaid] theme applied', scheme)
  return true
}

function loadMermaid(): Promise<MermaidModule> {
  if (!mermaidModulePromise) {
    mermaidModulePromise = import('mermaid').then(mod => {
      const api = mod.default
      applyHostMermaidTheme(api)
      return api
    })
  }
  return mermaidModulePromise.then(api => {
    applyHostMermaidTheme(api)
    return api
  })
}

type MermaidHostState = {
  boundConfig: string
  themeScheme: 'light' | 'dark'
  viewportGate: ViewportDeferral | null
  pending: boolean
}

/** Sanitized SVG markup cache — survives v-html host recreation while trailing text streams. */
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

export type MarkdownMermaidOptions = {
  /** When true, never render Mermaid — show a pending placeholder only. */
  isStreaming?: () => boolean
}

/** Mount Mermaid diagrams for `.md-mermaid` hosts inside markdown HTML. */
export function useMarkdownMermaid(
  rootRef: Ref<HTMLElement | null>,
  getTickSource: () => string,
  options: MarkdownMermaidOptions = {}
) {
  const hosts = new Map<HTMLElement, MermaidHostState>()
  const isStreaming = () => options.isStreaming?.() === true
  let themeObserver: MutationObserver | null = null
  let lastObservedScheme = mermaidThemeScheme()

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
    const toolbar = host.querySelector('.md-mermaid-toolbar')
    if (!(toolbar instanceof HTMLElement) || toolbar.dataset.ready === '1') return
    toolbar.dataset.ready = '1'

    const copyBtn = document.createElement('button')
    copyBtn.type = 'button'
    copyBtn.className = 'md-mermaid-btn'
    copyBtn.title = t('chat.md.copySource')
    copyBtn.setAttribute('aria-label', t('chat.md.copySource'))
    copyBtn.innerHTML = copyIconSvg
    copyBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const encoded = host.getAttribute('data-mermaid-config')
      const raw = encoded ? decodeSvgConfigAttr(encoded) : null
      if (!raw) {
        console.warn('[markdownMermaid] copy: missing config')
        return
      }
      void navigator.clipboard.writeText(raw).then(() => flashButton(copyBtn, t('common.copied'))).catch(err => {
        console.error('[markdownMermaid] copy failed', err)
      })
    })

    const downloadBtn = document.createElement('button')
    downloadBtn.type = 'button'
    downloadBtn.className = 'md-mermaid-btn'
    downloadBtn.title = t('chat.md.exportSvg')
    downloadBtn.setAttribute('aria-label', t('chat.md.exportSvg'))
    downloadBtn.innerHTML = downloadIconSvg
    downloadBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const encoded = host.getAttribute('data-mermaid-config')
      const svg = encoded ? cleanedSvgByConfig.get(mermaidThemeCacheKey(encoded)) : null
      if (!svg) {
        console.warn('[markdownMermaid] export: not rendered yet')
        return
      }
      const dataUrl = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg)
      const fileName = `pointer-diagram-${Date.now()}.svg`
      void saveDataUrlAsFile(dataUrl, fileName, [{ name: 'SVG', extensions: ['svg'] }])
        .then(result => {
          if (result === 'saved') flashButton(downloadBtn, t('chat.md.exported'))
        })
        .catch(err => {
          console.error('[markdownMermaid] export failed', err)
        })
    })

    const sourceBtn = document.createElement('button')
    sourceBtn.type = 'button'
    sourceBtn.className = 'md-mermaid-btn'
    sourceBtn.title = t('chat.md.viewSource')
    sourceBtn.setAttribute('aria-label', t('chat.md.viewSource'))
    sourceBtn.innerHTML = codeIconSvg
    sourceBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const pre = host.querySelector('.md-mermaid-source')
      if (!(pre instanceof HTMLElement)) return
      const open = pre.hasAttribute('hidden')
      if (open) {
        const encoded = host.getAttribute('data-mermaid-config')
        const raw = encoded ? decodeSvgConfigAttr(encoded) : ''
        pre.textContent = raw || ''
        pre.removeAttribute('hidden')
        sourceBtn.title = t('chat.md.hideSource')
        sourceBtn.setAttribute('aria-label', t('chat.md.hideSource'))
        sourceBtn.classList.add('md-mermaid-btn-active')
      } else {
        pre.setAttribute('hidden', '')
        sourceBtn.title = t('chat.md.viewSource')
        sourceBtn.setAttribute('aria-label', t('chat.md.viewSource'))
        sourceBtn.classList.remove('md-mermaid-btn-active')
      }
    })

    const zoomBtn = document.createElement('button')
    zoomBtn.type = 'button'
    zoomBtn.className = 'md-mermaid-btn'
    zoomBtn.title = t('chat.md.zoomView')
    zoomBtn.setAttribute('aria-label', t('chat.md.zoomView'))
    zoomBtn.innerHTML = zoomIconSvg
    zoomBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const frame = host.querySelector('.md-mermaid-frame')
      const svg = frame?.querySelector('svg')
      if (svg instanceof SVGElement) openDiagramZoom(svg)
    })

    toolbar.append(zoomBtn, copyBtn, downloadBtn, sourceBtn)
  }

  function setToolbarVisible(host: HTMLElement, visible: boolean) {
    const toolbar = host.querySelector('.md-mermaid-toolbar')
    if (!(toolbar instanceof HTMLElement)) return
    toolbar.hidden = !visible
  }

  function showStatus(host: HTMLElement, message: string, kind: 'pending' | 'error') {
    const frame = host.querySelector('.md-mermaid-frame')
    hosts.delete(host)
    host.classList.toggle('md-mermaid--invalid', kind === 'error')
    host.classList.toggle('md-mermaid--pending', kind === 'pending')
    setToolbarVisible(host, kind !== 'pending')
    if (!(frame instanceof HTMLElement)) return
    frame.replaceChildren()
    const status = document.createElement('div')
    status.className = `md-mermaid-status md-mermaid-status-${kind}`
    status.textContent = message
    frame.appendChild(status)
  }

  function mountOrUpdate(host: HTMLElement) {
    void mountOrUpdateAsync(host)
  }

  async function mountOrUpdateAsync(host: HTMLElement) {
    ensureToolbar(host)

    const encoded = host.getAttribute('data-mermaid-config')
    if (!encoded) {
      showStatus(host, t('chat.md.diagramMissing'), 'error')
      return
    }
    let raw: string | null
    try {
      raw = decodeSvgConfigAttr(encoded)
    } catch {
      raw = null
    }
    if (raw == null) {
      showStatus(host, t('chat.md.diagramParseFail'), 'error')
      return
    }

    // Defer rendering until the assistant turn finishes: Mermaid render is
    // comparatively expensive and only the final source should be laid out.
    if (isStreaming() || isStreamingMermaidStub(raw)) {
      showStatus(host, t('chat.md.diagramPending'), 'pending')
      return
    }

    const prev = hosts.get(host)
    const scheme = mermaidThemeScheme()
    if (
      prev?.boundConfig === encoded &&
      prev.themeScheme === scheme &&
      host.querySelector('.md-mermaid-frame > svg')
    ) {
      setToolbarVisible(host, true)
      return
    }
    if (prev?.boundConfig === encoded && prev.pending) {
      return
    }

    const inView = isInViewForLazyMount(host)
    if (prev?.boundConfig === encoded && prev.viewportGate && !inView) {
      return
    }
    if (!inView) {
      prev?.viewportGate?.disconnect()
      const viewportGate = deferUntilInView(host, () => {
        const state = hosts.get(host)
        if (state) state.viewportGate = null
        if (import.meta.env.DEV) {
          console.info('[markdownMermaid] diagram entered view; mount')
        }
        mountOrUpdate(host)
      })
      hosts.set(host, {
        boundConfig: encoded,
        themeScheme: mermaidThemeScheme(),
        viewportGate,
        pending: false,
      })
      return
    }

    prev?.viewportGate?.disconnect()
    hosts.set(host, {
      boundConfig: encoded,
      themeScheme: mermaidThemeScheme(),
      viewportGate: null,
      pending: true,
    })

    const cacheKey = mermaidThemeCacheKey(encoded)
    let cleaned = cleanedSvgByConfig.get(cacheKey)
    if (!cleaned) {
      try {
        const mermaid = await loadMermaid()
        // Host may have been torn down / reconfigured while Mermaid was loading.
        if (host.getAttribute('data-mermaid-config') !== encoded) {
          const state = hosts.get(host)
          if (state?.boundConfig === encoded) state.pending = false
          return
        }
        if (mermaidThemeScheme() !== scheme) {
          const state = hosts.get(host)
          if (state) state.pending = false
          mountOrUpdate(host)
          return
        }
        const source = prepareMermaidSource(raw)
        if (!source) {
          const state = hosts.get(host)
          if (state) state.pending = false
          console.warn('[markdownMermaid] source empty after stripping theme directives')
          showStatus(host, t('chat.md.diagramSyntax'), 'error')
          return
        }
        if (!isSupportedMermaidSource(mermaid, source)) {
          const state = hosts.get(host)
          if (state) state.pending = false
          showStatus(host, t('chat.md.diagramUnsupported'), 'error')
          return
        }
        const holder = document.createElement('div')
        holder.id = `md-mermaid-${renderSeq++}`
        holder.style.display = 'none'
        document.body.appendChild(holder)
        let svg: string
        try {
          const result = await mermaid.render(holder.id, source)
          svg = result.svg
        } finally {
          holder.remove()
        }
        const parsed = sanitizeSvgMarkup(svg, { allowForeignObject: true })
        if (!parsed.ok) {
          const state = hosts.get(host)
          if (state) state.pending = false
          showStatus(host, t('chat.md.diagramRenderFail'), 'error')
          return
        }
        cleaned = roundMermaidSvgRects(parsed.svg)
        cacheCleanedSvg(cacheKey, cleaned)
      } catch (err) {
        console.error('[markdownMermaid] render failed', err)
        const state = hosts.get(host)
        if (state) state.pending = false
        showStatus(host, t('chat.md.diagramSyntax'), 'error')
        return
      }
    }

    if (!isInViewForLazyMount(host)) {
      const state = hosts.get(host)
      if (state) state.pending = false
      console.info('[markdownMermaid] left viewport before insert; defer')
      mountOrUpdate(host)
      return
    }

    host.classList.remove('md-mermaid--invalid', 'md-mermaid--pending')
    setToolbarVisible(host, true)
    let frame = host.querySelector('.md-mermaid-frame')
    if (!(frame instanceof HTMLElement)) {
      frame = document.createElement('div')
      frame.className = 'md-mermaid-frame'
      host.appendChild(frame)
    }
    frame.replaceChildren()

    try {
      if (typeof DOMParser === 'undefined') {
        frame.textContent = 'SVG'
        console.warn('[markdownMermaid] DOMParser unavailable; skip mount')
        return
      }
      const doc = new DOMParser().parseFromString(cleaned, 'image/svg+xml')
      const parseError = doc.querySelector('parsererror')
      if (parseError) {
        showStatus(host, t('chat.md.diagramInvalid'), 'error')
        return
      }
      const root = doc.documentElement
      if (!root || root.localName.toLowerCase() !== 'svg') {
        showStatus(host, t('chat.md.diagramInvalid'), 'error')
        return
      }
      const imported = document.importNode(root, true)
      if (!(imported instanceof SVGElement)) {
        showStatus(host, t('chat.md.diagramInvalid'), 'error')
        return
      }
      imported.setAttribute('role', 'img')
      if (!imported.getAttribute('aria-label')) {
        imported.setAttribute('aria-label', 'diagram')
      }
      frame.appendChild(imported)
      applySvgMountLayout(imported, { cropToContent: true })
      cacheCleanedSvg(cacheKey, imported.outerHTML)
      // Mermaid foreignObject labels / fonts can settle after first paint; CTM-aware
      // crop needs a laid-out SVG. Re-crop once layout is ready.
      requestAnimationFrame(() => {
        if (!imported.isConnected) return
        applySvgMountLayout(imported, { cropToContent: true })
        cacheCleanedSvg(cacheKey, imported.outerHTML)
      })
      imported.addEventListener('click', e => {
        e.preventDefault()
        e.stopPropagation()
        openDiagramZoom(imported)
      })
      hosts.set(host, {
        boundConfig: encoded,
        themeScheme: mermaidThemeScheme(),
        viewportGate: null,
        pending: false,
      })
      host.dataset.mermaidBound = encoded
      console.info('[markdownMermaid] mounted mermaid host')
    } catch (err) {
      console.error('[markdownMermaid] mount failed', err)
      const state = hosts.get(host)
      if (state) state.pending = false
      showStatus(host, t('chat.md.diagramRenderFail'), 'error')
    }
  }

  function sync() {
    const root = rootRef.value
    if (!root) return
    const found = root.querySelectorAll('.md-mermaid')
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
          console.warn('[markdownMermaid] viewportGate disconnect failed', err)
        }
        hosts.delete(host)
      }
    }
  }

  function onDocumentThemeClassChange() {
    const scheme = mermaidThemeScheme()
    if (scheme === lastObservedScheme) return
    lastObservedScheme = scheme
    appliedThemeScheme = null
    cleanedSvgByConfig.clear()
    for (const state of hosts.values()) {
      state.boundConfig = ''
    }
    console.info('[markdownMermaid] html theme changed; remount diagrams', scheme)
    void nextTick(sync)
  }

  onMounted(() => {
    if (typeof MutationObserver !== 'undefined') {
      themeObserver = new MutationObserver(onDocumentThemeClassChange)
      themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['class'],
      })
    }
    void nextTick(sync)
  })
  onBeforeUnmount(() => {
    try {
      themeObserver?.disconnect()
    } catch (err) {
      console.warn('[markdownMermaid] theme observer disconnect failed', err)
    }
    themeObserver = null
    for (const state of hosts.values()) {
      try {
        state.viewportGate?.disconnect()
      } catch (err) {
        console.warn('[markdownMermaid] viewportGate disconnect failed', err)
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
