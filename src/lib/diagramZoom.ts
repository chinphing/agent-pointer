import { t } from '../i18n'
import { trackFullscreenExit } from './fullscreenTrack'

/**
 * Full-screen zoom overlay for rendered diagrams (Mermaid / SVG fence / Chart.js).
 *
 * The overlay is imperative DOM with inline styles so it works from the
 * composable layer (`useMarkdownMermaid`, `useMarkdownSvgs`,
 * `useMarkdownCharts`) without depending on Tailwind JIT scanning dynamic
 * class names.
 *
 * - SVG sources are deep-cloned; the original node stays in place. Mermaid's
 *   embedded `<style>` rules use `#<svgId>` prefixed selectors, so the clone
 *   gets a fresh id and the style text is rewritten to match.
 * - Canvas sources are exported to a PNG data URL (`cloneNode` would lose the
 *   pixels).
 * - Only one overlay is open at a time.
 */

export const zoomIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>`

export const OVERLAY_CLASS = 'diagram-zoom-overlay'

function parseViewBox(
  viewBox: string | null,
): { width: number; height: number } | null {
  if (!viewBox) return null
  const parts = viewBox.trim().split(/[\s,]+/).map(Number)
  if (parts.length !== 4 || parts.some(n => !Number.isFinite(n))) return null
  const width = parts[2]
  const height = parts[3]
  if (width <= 0 || height <= 0) return null
  return { width, height }
}

let activeClose: (() => void) | null = null
let cloneSeq = 0
let unlistenFullscreen: (() => void) | undefined

/** Close the open zoom overlay, if any. */
export function closeDiagramZoom(): void {
  activeClose?.()
  activeClose = null
}

/**
 * Open a full-screen zoom overlay for a rendered diagram.
 * Returns a close function (idempotent).
 */
export function openDiagramZoom(source: Element): () => void {
  closeDiagramZoom()

  const overlay = document.createElement('div')
  overlay.className = OVERLAY_CLASS
  Object.assign(overlay.style, {
    position: 'fixed',
    inset: '0',
    zIndex: '9999',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'hsl(var(--foreground) / 0.32)',
    backdropFilter: 'blur(4px)',
    overflow: 'hidden',
    userSelect: 'none',
    cursor: 'grab',
  })

  const stage = document.createElement('div')
  Object.assign(stage.style, {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    maxWidth: '90vw',
    maxHeight: '90vh',
  })
  overlay.appendChild(stage)

  // ── content ──────────────────────────────────────────────────────────────
  // Determine the diagram's natural (un-scaled) size, then fit it into the
  // viewport on open. Without this, an SVG's own `width="100%"` collapses to
  // min-content inside the flex stage and the diagram opens tiny.
  let content: HTMLElement
  let naturalW = 0
  let naturalH = 0
  if (source instanceof HTMLCanvasElement) {
    const img = document.createElement('img')
    img.src = source.toDataURL('image/png')
    img.draggable = false
    content = img
    naturalW = source.width
    naturalH = source.height
  } else {
    content = source.cloneNode(true) as HTMLElement
    const sourceId = source.getAttribute('id')
    if (sourceId) {
      const newId = `diagram-zoom-${++cloneSeq}`
      content.setAttribute('id', newId)
      // Mermaid embeds `<style>` rules scoped to the original svg id
      // (e.g. `#g1 .node …`); rewrite them so the clone keeps its styling.
      for (const styleEl of content.querySelectorAll('style')) {
        styleEl.textContent = styleEl.textContent.split(`#${sourceId}`).join(`#${newId}`)
      }
    }
    // Host mermaid paints target `.diagram-zoom-overlay svg` (see globals.css).
    content.classList.add('diagram-zoom-svg')
    const vb = parseViewBox(content.getAttribute('viewBox'))
    if (vb) {
      naturalW = vb.width
      naturalH = vb.height
    } else {
      const rect = content.getBoundingClientRect()
      naturalW = rect.width
      naturalH = rect.height
    }
  }
  if (naturalW > 0 && naturalH > 0) {
    content.style.width = `${naturalW}px`
    content.style.height = `${naturalH}px`
  }
  Object.assign(content.style, {
    maxWidth: 'none',
    maxHeight: 'none',
    objectFit: 'contain',
    borderRadius: '8px',
    boxShadow: '0 12px 40px hsl(var(--foreground) / 0.18)',
    background: 'hsl(var(--fence-bg))',
  })
  stage.appendChild(content)

  // Fit into ~90% of the viewport, never upscale past the natural size.
  const fitScale =
    naturalW > 0 && naturalH > 0
      ? Math.min(
          (window.innerWidth * 0.9) / naturalW,
          (window.innerHeight * 0.9) / naturalH,
          1,
        )
      : 1

  // ── zoom indicator + hint ────────────────────────────────────────────────
  const indicator = document.createElement('div')
  indicator.className = 'diagram-zoom-indicator'
  Object.assign(indicator.style, {
    position: 'fixed',
    bottom: '24px',
    left: '50%',
    transform: 'translateX(-50%)',
    padding: '4px 12px',
    borderRadius: '9999px',
    background: 'hsl(var(--foreground) / 0.72)',
    color: 'hsl(var(--background) / 0.9)',
    fontSize: '12px',
    pointerEvents: 'none',
    fontFamily: 'system-ui, sans-serif',
  })
  overlay.appendChild(indicator)

  const hint = document.createElement('div')
  hint.className = 'diagram-zoom-hint'
  hint.textContent = t('markdown.zoomHint')
  Object.assign(hint.style, {
    position: 'fixed',
    top: '16px',
    right: '16px',
    padding: '6px 12px',
    borderRadius: '8px',
    background: 'hsl(var(--card) / 0.92)',
    color: 'hsl(var(--muted))',
    fontSize: '12px',
    pointerEvents: 'none',
    fontFamily: 'system-ui, sans-serif',
  })
  overlay.appendChild(hint)

  // ── pan / zoom state ─────────────────────────────────────────────────────
  let scale = fitScale
  let tx = 0
  let ty = 0
  let dragging = false
  let dragStartX = 0
  let dragStartY = 0
  let dragOriginX = 0
  let dragOriginY = 0

  function applyTransform() {
    stage.style.transform = `translate(${tx}px, ${ty}px) scale(${scale})`
    indicator.textContent = `${Math.round(scale * 100)}%`
  }

  function resetZoom() {
    scale = fitScale
    tx = 0
    ty = 0
    dragging = false
    applyTransform()
    overlay.style.cursor = 'grab'
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault()
    const delta = e.deltaY > 0 ? 0.9 : 1.1
    const newScale = Math.min(Math.max(scale * delta, 0.25), 10)
    // zoom toward cursor position (relative to overlay center)
    const rect = overlay.getBoundingClientRect()
    const cx = e.clientX - rect.left - rect.width / 2
    const cy = e.clientY - rect.top - rect.height / 2
    tx = cx - (cx - tx) * (newScale / scale)
    ty = cy - (cy - ty) * (newScale / scale)
    scale = newScale
    applyTransform()
  }

  function onMouseDown(e: MouseEvent) {
    if (scale <= 1) return
    dragging = true
    dragStartX = e.clientX
    dragStartY = e.clientY
    dragOriginX = tx
    dragOriginY = ty
    overlay.style.cursor = 'grabbing'
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return
    tx = dragOriginX + (e.clientX - dragStartX)
    ty = dragOriginY + (e.clientY - dragStartY)
    applyTransform()
  }

  function onMouseUp() {
    dragging = false
    overlay.style.cursor = scale > 1 ? 'grab' : 'grab'
  }

  function onBackdropClick(e: MouseEvent) {
    if (e.target === overlay) close()
  }

  function onDblClick(e: MouseEvent) {
    e.stopPropagation()
    resetZoom()
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // Capture-phase + stopImmediatePropagation: while the overlay is open,
      // Esc must not leak to other document/window listeners (DiffView
      // maximized, TerminalLiveOutputModal fullscreen, other modals…).
      e.stopImmediatePropagation()
      close()
    }
    if (e.key === 'r' || e.key === 'R') {
      e.stopImmediatePropagation()
      resetZoom()
    }
  }

  function close() {
    overlay.remove()
    document.removeEventListener('keydown', onKeydown, true)
    unlistenFullscreen?.()
    unlistenFullscreen = undefined
    if (activeClose === close) activeClose = null
  }

  overlay.addEventListener('wheel', onWheel, { passive: false })
  overlay.addEventListener('mousedown', onMouseDown)
  overlay.addEventListener('mousemove', onMouseMove)
  overlay.addEventListener('mouseup', onMouseUp)
  overlay.addEventListener('mouseleave', onMouseUp)
  overlay.addEventListener('click', onBackdropClick)
  stage.addEventListener('dblclick', onDblClick)
  document.addEventListener('keydown', onKeydown, true)

  document.body.appendChild(overlay)
  applyTransform()
  // On desktop, Esc while the OS-level window is fullscreen is consumed by the
  // system to exit fullscreen — the DOM keydown never reaches the page. Close
  // the overlay when the window leaves fullscreen so the user's Esc intent
  // still dismisses the zoom.
  void trackFullscreenExit(close).then(fn => {
    unlistenFullscreen = fn
  })
  activeClose = close
  return close
}
