import { nextTick, onBeforeUnmount, onMounted, watch, type Ref } from 'vue'
import type { Chart as ChartInstance } from 'chart.js'
import {
  applyChartTheme,
  decodeChartConfigAttr,
  exportChartCanvasPngDataUrl,
  tryParseChartConfig,
} from '../lib/markdownChart'
import { saveDataUrlAsFile } from '../lib/saveLocalFile'
import { openDiagramZoom, zoomIconSvg } from '../lib/diagramZoom'

const copyIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`
const checkIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`
const downloadIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>`
const codeIconSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>`

type ChartCtor = typeof import('chart.js/auto').Chart
let chartCtorPromise: Promise<ChartCtor> | null = null

function loadChartCtor(): Promise<ChartCtor> {
  if (!chartCtorPromise) {
    chartCtorPromise = import('chart.js/auto').then(mod => mod.Chart)
  }
  return chartCtorPromise
}

type ChartHostState = {
  chart: ChartInstance | null
  boundConfig: string
  resizeObserver: ResizeObserver | null
  pending: boolean
}

export type MarkdownChartsOptions = {
  /** When true, never mount Chart.js — show a pending placeholder only. */
  isStreaming?: () => boolean
}

/** Mount Chart.js charts for `.md-chart` hosts inside markdown HTML. */
export function useMarkdownCharts(
  rootRef: Ref<HTMLElement | null>,
  getTickSource: () => string,
  options: MarkdownChartsOptions = {}
) {
  const hosts = new Map<HTMLElement, ChartHostState>()
  const isStreaming = () => options.isStreaming?.() === true
  let attachRaf = 0
  let domObserver: MutationObserver | null = null

  function hostIsLive(host: HTMLElement): boolean {
    const root = rootRef.value
    return !!root && host.isConnected && root.contains(host)
  }

  function measureChartBox(box: HTMLElement): { w: number; h: number } {
    const rect = box.getBoundingClientRect()
    const w = Math.max(0, Math.floor(box.clientWidth || rect.width))
    const h = Math.max(0, Math.floor(box.clientHeight || rect.height || 360))
    return { w, h }
  }

  function waitForChartBoxSize(
    box: HTMLElement,
    stillValid: () => boolean
  ): Promise<{ w: number; h: number }> {
    const first = measureChartBox(box)
    if (first.w >= 8 && first.h >= 8) return Promise.resolve(first)

    return new Promise(resolve => {
      let settled = false
      let ro: ResizeObserver | null = null
      const finish = (reason: string) => {
        if (settled) return
        settled = true
        try {
          ro?.disconnect()
        } catch (err) {
          console.warn('[markdownCharts] size observer disconnect failed', err)
        }
        const m = measureChartBox(box)
        console.info('[markdownCharts] chart box sized', reason, m)
        resolve(m)
      }

      if (typeof ResizeObserver !== 'undefined') {
        ro = new ResizeObserver(() => {
          if (!stillValid()) {
            finish('invalid')
            return
          }
          const m = measureChartBox(box)
          if (m.w >= 8 && m.h >= 8) finish('resize')
        })
        try {
          ro.observe(box)
        } catch (err) {
          console.warn('[markdownCharts] size observe failed', err)
        }
      }

      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          if (!stillValid()) {
            finish('invalid')
            return
          }
          const m = measureChartBox(box)
          if (m.w >= 8 && m.h >= 8) {
            finish('raf')
            return
          }
          window.setTimeout(() => {
            if (!stillValid()) {
              finish('invalid')
              return
            }
            const later = measureChartBox(box)
            if (later.w >= 8 && later.h >= 8) finish('timeout-ok')
            else {
              console.warn('[markdownCharts] chart box still unlaid-out', later)
              finish('timeout-small')
            }
          }, 250)
        })
      })
    })
  }

  function destroyHost(host: HTMLElement) {
    const state = hosts.get(host)
    if (state?.resizeObserver) {
      try {
        state.resizeObserver.disconnect()
      } catch (err) {
        console.warn('[markdownCharts] resizeObserver disconnect failed', err)
      }
    }
    if (state?.chart) {
      try {
        state.chart.destroy()
      } catch (err) {
        console.warn('[markdownCharts] destroy failed', err)
      }
    }
    hosts.delete(host)
  }

  function extractDatasetValues(chart: ChartInstance): number[] {
    const rawPoints = chart.data.datasets[0]?.data
    if (!Array.isArray(rawPoints)) return []
    const values: number[] = []
    for (const point of rawPoints) {
      if (typeof point === 'number' && Number.isFinite(point)) values.push(point)
      else if (point && typeof point === 'object' && 'y' in point) {
        const y = (point as { y?: unknown }).y
        if (typeof y === 'number' && Number.isFinite(y)) values.push(y)
      }
    }
    return values
  }

  /**
   * Detect squashed series: data should sit mid/high on the axis but pixels
   * stay on the floor (seen on macOS WKWebView with bad first layout).
   */
  function chartPixelsLookSquashed(chart: ChartInstance): boolean {
    try {
      const yScale = chart.scales.y
      if (!yScale || !Number.isFinite(yScale.min) || !Number.isFinite(yScale.max)) return false
      const span = yScale.max - yScale.min
      if (!(span > 0)) return false
      const meta = chart.getDatasetMeta(0)
      if (!meta?.data?.length) return false
      const values = extractDatasetValues(chart)
      if (values.length < 2) return false
      const dataMax = Math.max(...values)
      const dataMin = Math.min(...values)
      const idx = values.indexOf(dataMax)
      const py = meta.data[idx]?.getProps(['y'], true).y
      if (typeof py !== 'number' || !Number.isFinite(py)) return false
      const expectedFrac = (dataMax - yScale.min) / span
      const area = yScale.bottom - yScale.top
      if (!(area > 8)) return false
      const actualFrac = (yScale.bottom - py) / area
      console.info('[markdownCharts] scale check', {
        dataMin,
        dataMax,
        scaleMin: yScale.min,
        scaleMax: yScale.max,
        expectedFrac,
        actualFrac,
        top: yScale.top,
        bottom: yScale.bottom,
      })
      // Data belongs in the upper half, but the point is stuck near the floor.
      return expectedFrac > 0.55 && actualFrac < 0.3 && dataMax - dataMin > span * 0.2
    } catch (err) {
      console.warn('[markdownCharts] squash check failed', err)
      return false
    }
  }

  function repairSquashedChart(chart: ChartInstance) {
    const y = chart.options.scales?.y as Record<string, unknown> | undefined
    if (y) {
      delete y.min
      delete y.max
      delete y.suggestedMin
      delete y.suggestedMax
      delete y.grace
      if (isPlainObject(y.ticks)) {
        const ticks = { ...(y.ticks as Record<string, unknown>) }
        delete ticks.stepSize
        y.ticks = ticks
      }
    }
    // Re-assert primitive numeric data (WebKit parse quirks).
    for (const ds of chart.data.datasets) {
      if (!ds || !Array.isArray(ds.data)) continue
      ds.data = ds.data.map((v: unknown) => {
        if (typeof v === 'number' && Number.isFinite(v)) return v
        if (v && typeof v === 'object' && 'y' in v) {
          const yv = (v as { y?: unknown }).y
          return typeof yv === 'number' && Number.isFinite(yv) ? yv : null
        }
        const n = Number.parseFloat(String(v))
        return Number.isFinite(n) ? n : null
      }) as never
    }
    chart.update('none')
    chart.resize()
  }

  function isPlainObject(v: unknown): v is Record<string, unknown> {
    return !!v && typeof v === 'object' && !Array.isArray(v)
  }

  function pruneDetached() {
    const root = rootRef.value
    for (const host of [...hosts.keys()]) {
      if (!root || !root.contains(host)) destroyHost(host)
    }
  }

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
    const toolbar = host.querySelector('.md-chart-toolbar')
    if (!(toolbar instanceof HTMLElement) || toolbar.dataset.ready === '1') return
    toolbar.dataset.ready = '1'

    const copyBtn = document.createElement('button')
    copyBtn.type = 'button'
    copyBtn.className = 'md-chart-btn'
    copyBtn.title = '复制配置'
    copyBtn.setAttribute('aria-label', '复制配置')
    copyBtn.innerHTML = copyIconSvg
    copyBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const encoded = host.getAttribute('data-chart-config')
      const raw = encoded ? decodeChartConfigAttr(encoded) : null
      if (!raw) {
        console.warn('[markdownCharts] copy: missing config')
        return
      }
      let text = raw
      try {
        text = JSON.stringify(JSON.parse(raw), null, 2)
      } catch {
        /* keep raw */
      }
      void navigator.clipboard.writeText(text).then(() => flashButton(copyBtn, '已复制')).catch(err => {
        console.error('[markdownCharts] copy failed', err)
      })
    })

    const downloadBtn = document.createElement('button')
    downloadBtn.type = 'button'
    downloadBtn.className = 'md-chart-btn'
    downloadBtn.title = '导出图片'
    downloadBtn.setAttribute('aria-label', '导出图片')
    downloadBtn.innerHTML = downloadIconSvg
    downloadBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const state = hosts.get(host)
      const canvas =
        state?.chart?.canvas ??
        host.querySelector('canvas')
      if (!(canvas instanceof HTMLCanvasElement)) {
        console.warn('[markdownCharts] export: no canvas')
        return
      }
      try {
        // Prefer live Chart instance so pixels match what the user sees.
        if (state?.chart) {
          try {
            state.chart.resize()
          } catch (err) {
            console.warn('[markdownCharts] export resize failed', err)
          }
        }
        const url = exportChartCanvasPngDataUrl(canvas)
        const fileName = `pointer-chart-${Date.now()}.png`
        void saveDataUrlAsFile(url, fileName, [{ name: 'PNG', extensions: ['png'] }])
          .then(result => {
            if (result === 'saved') flashButton(downloadBtn, '已导出')
          })
          .catch(err => {
            console.error('[markdownCharts] export failed', err)
          })
      } catch (err) {
        console.error('[markdownCharts] export failed', err)
      }
    })

    const sourceBtn = document.createElement('button')
    sourceBtn.type = 'button'
    sourceBtn.className = 'md-chart-btn'
    sourceBtn.title = '查看配置'
    sourceBtn.setAttribute('aria-label', '查看配置')
    sourceBtn.innerHTML = codeIconSvg
    sourceBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const pre = host.querySelector('.md-chart-source')
      if (!(pre instanceof HTMLElement)) return
      const open = pre.hasAttribute('hidden')
      if (open) {
        const encoded = host.getAttribute('data-chart-config')
        const raw = encoded ? decodeChartConfigAttr(encoded) : ''
        let text = raw || ''
        try {
          text = JSON.stringify(JSON.parse(text), null, 2)
        } catch {
          /* keep raw */
        }
        pre.textContent = text
        pre.removeAttribute('hidden')
        sourceBtn.title = '隐藏配置'
        sourceBtn.setAttribute('aria-label', '隐藏配置')
        sourceBtn.classList.add('md-chart-btn-active')
      } else {
        pre.setAttribute('hidden', '')
        sourceBtn.title = '查看配置'
        sourceBtn.setAttribute('aria-label', '查看配置')
        sourceBtn.classList.remove('md-chart-btn-active')
      }
    })

    const zoomBtn = document.createElement('button')
    zoomBtn.type = 'button'
    zoomBtn.className = 'md-chart-btn'
    zoomBtn.title = '放大查看'
    zoomBtn.setAttribute('aria-label', '放大查看')
    zoomBtn.innerHTML = zoomIconSvg
    zoomBtn.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      const canvas = host.querySelector('.md-chart-canvas-wrap canvas')
      if (canvas instanceof HTMLCanvasElement) openDiagramZoom(canvas)
    })

    toolbar.append(zoomBtn, copyBtn, downloadBtn, sourceBtn)
  }

  function setToolbarVisible(host: HTMLElement, visible: boolean) {
    const toolbar = host.querySelector('.md-chart-toolbar')
    if (!(toolbar instanceof HTMLElement)) return
    toolbar.hidden = !visible
  }

  function showStatus(host: HTMLElement, message: string, kind: 'pending' | 'error') {
    const wrap = host.querySelector('.md-chart-canvas-wrap')
    const existing =
      wrap instanceof HTMLElement
        ? wrap.querySelector(`.md-chart-status-${kind}`)
        : null
    // Streaming placeholder HTML already contains the pending status — avoid flicker.
    if (
      existing instanceof HTMLElement &&
      existing.textContent === message &&
      (kind === 'pending' ? host.classList.contains('md-chart--pending') : true)
    ) {
      setToolbarVisible(host, kind !== 'pending')
      return
    }

    destroyHost(host)
    host.classList.toggle('md-chart--invalid', kind === 'error')
    host.classList.toggle('md-chart--pending', kind === 'pending')
    setToolbarVisible(host, kind !== 'pending')
    if (!(wrap instanceof HTMLElement)) return
    wrap.replaceChildren()
    const status = document.createElement('div')
    status.className = `md-chart-status md-chart-status-${kind}`
    status.textContent = message
    wrap.appendChild(status)
  }

  function mountOrUpdate(host: HTMLElement) {
    void mountOrUpdateAsync(host)
  }

  async function mountOrUpdateAsync(host: HTMLElement) {
    // Defer Chart.js until the assistant turn finishes streaming.
    if (isStreaming()) {
      showStatus(host, '图表生成中…', 'pending')
      return
    }

    ensureToolbar(host)

    const encoded = host.getAttribute('data-chart-config')
    if (!encoded) {
      showStatus(host, '图表配置缺失', 'error')
      return
    }
    const raw = decodeChartConfigAttr(encoded)
    if (raw == null) {
      showStatus(host, '图表配置无法解析', 'error')
      return
    }
    const parsed = tryParseChartConfig(raw)
    if (!parsed.ok) {
      const pending = parsed.reason === 'invalid_json' || parsed.reason === 'empty'
      showStatus(
        host,
        pending ? '图表生成中…' : '图表配置无效',
        pending ? 'pending' : 'error'
      )
      return
    }

    const prev = hosts.get(host)
    if (prev?.boundConfig === encoded && prev.chart) {
      setToolbarVisible(host, true)
      return
    }
    // Chart.js is a lazy chunk — a second attach during import must not wipe the canvas.
    if (prev?.boundConfig === encoded && prev.pending) {
      return
    }
    // 0-size box: ResizeObserver will schedule attach; do not rebuild the host.
    if (prev?.boundConfig === encoded && !prev.chart && prev.resizeObserver) {
      return
    }

    if (prev?.chart) {
      try {
        prev.chart.destroy()
      } catch (err) {
        console.warn('[markdownCharts] rebuild destroy failed', err)
      }
    }
    if (prev?.resizeObserver) {
      try {
        prev.resizeObserver.disconnect()
      } catch (err) {
        console.warn('[markdownCharts] rebuild observer disconnect failed', err)
      }
    }

    host.classList.remove('md-chart--invalid', 'md-chart--pending')
    setToolbarVisible(host, true)
    let wrap = host.querySelector('.md-chart-canvas-wrap')
    if (!(wrap instanceof HTMLElement)) {
      wrap = document.createElement('div')
      wrap.className = 'md-chart-canvas-wrap'
      host.appendChild(wrap)
    }
    wrap.innerHTML = ''
    const box = document.createElement('div')
    box.className = 'md-chart-canvas-box'
    const canvas = document.createElement('canvas')
    canvas.setAttribute('role', 'img')
    canvas.setAttribute('aria-label', 'chart')
    box.appendChild(canvas)
    canvas.addEventListener('click', e => {
      e.preventDefault()
      e.stopPropagation()
      openDiagramZoom(canvas)
    })
    wrap.appendChild(box)
    hosts.set(host, { chart: null, boundConfig: encoded, resizeObserver: null, pending: true })

    const stillValid = () =>
      hostIsLive(host) &&
      host.getAttribute('data-chart-config') === encoded &&
      hosts.get(host)?.pending === true &&
      hosts.get(host)?.boundConfig === encoded

    try {
      const Chart = await loadChartCtor()
      if (!stillValid()) {
        if (!hostIsLive(host) || host.getAttribute('data-chart-config') !== encoded) {
          console.warn('[markdownCharts] host replaced while Chart.js loaded; retry attach')
          scheduleAttach()
        }
        return
      }

      const themed = applyChartTheme(parsed.config)
      const size = await waitForChartBoxSize(box, stillValid)
      if (!stillValid()) {
        if (!hostIsLive(host) || host.getAttribute('data-chart-config') !== encoded) {
          console.warn('[markdownCharts] host replaced while waiting for layout; retry attach')
          scheduleAttach()
        }
        return
      }
      if (size.w < 8 || size.h < 8) {
        console.warn('[markdownCharts] skip Chart.js on 0-size box; wait for layout', size)
        const layoutObserver =
          typeof ResizeObserver !== 'undefined'
            ? new ResizeObserver(() => {
                const next = measureChartBox(box)
                if (next.w < 8 || next.h < 8) return
                try {
                  layoutObserver?.disconnect()
                } catch (err) {
                  console.warn('[markdownCharts] layout observer disconnect failed', err)
                }
                const state = hosts.get(host)
                if (state) {
                  state.pending = false
                  state.resizeObserver = null
                }
                console.info('[markdownCharts] chart box became visible; remount')
                scheduleAttach()
              })
            : null
        layoutObserver?.observe(box)
        hosts.set(host, {
          chart: null,
          boundConfig: encoded,
          resizeObserver: layoutObserver,
          pending: false,
        })
        return
      }

      const chart = new Chart(canvas, themed as never)
      const resizeObserver =
        typeof ResizeObserver !== 'undefined'
          ? new ResizeObserver(() => {
              try {
                chart.resize()
              } catch (err) {
                console.warn('[markdownCharts] resizeObserver resize failed', err)
              }
            })
          : null
      resizeObserver?.observe(box)
      hosts.set(host, { chart, boundConfig: encoded, resizeObserver, pending: false })
      host.dataset.chartBound = encoded
      const finish = () => {
        try {
          if (!hostIsLive(host)) return
          chart.resize()
          if (chartPixelsLookSquashed(chart)) {
            console.warn('[markdownCharts] squashed series on WKWebView; repairing scale')
            repairSquashedChart(chart)
            if (chartPixelsLookSquashed(chart)) {
              console.error('[markdownCharts] series still squashed after repair', {
                data: chart.data.datasets[0]?.data,
              })
            }
          }
        } catch (err) {
          console.warn('[markdownCharts] post-mount layout failed', err)
        }
      }
      // Two frames: first layout after paint, second after WebKit settles canvas backing store.
      requestAnimationFrame(() => requestAnimationFrame(finish))
      const sample = (themed.data as { datasets?: { data?: unknown[] }[] })?.datasets?.[0]?.data
      console.info(
        '[markdownCharts] mounted',
        themed.type,
        Array.isArray(sample) ? sample.slice(0, 3) : null
      )
    } catch (err) {
      console.error('[markdownCharts] Chart.js failed', err)
      const state = hosts.get(host)
      if (state) state.pending = false
      showStatus(host, '图表渲染失败', 'error')
    }
  }

  function attach() {
    const root = rootRef.value
    if (!root) return
    pruneDetached()
    root.querySelectorAll('.md-chart').forEach(node => {
      if (node instanceof HTMLElement) mountOrUpdate(node)
    })
  }

  function scheduleAttach() {
    if (attachRaf) return
    attachRaf = window.requestAnimationFrame(() => {
      attachRaf = 0
      attach()
    })
  }

  function bindDomObserver(root: HTMLElement | null) {
    if (domObserver) {
      try {
        domObserver.disconnect()
      } catch (err) {
        console.warn('[markdownCharts] dom observer disconnect failed', err)
      }
      domObserver = null
    }
    if (!root || typeof MutationObserver === 'undefined') return
    // v-html / virtual-list patches replace .md-chart hosts without changing
    // markdown source — remount so Chart.js is not left on a detached canvas.
    domObserver = new MutationObserver(() => {
      scheduleAttach()
    })
    try {
      domObserver.observe(root, { childList: true, subtree: true })
    } catch (err) {
      console.warn('[markdownCharts] dom observe failed', err)
    }
  }

  onMounted(() => {
    nextTick(attach)
  })

  watch(
    rootRef,
    el => {
      bindDomObserver(el)
      nextTick(attach)
    }
  )

  watch(getTickSource, () => {
    nextTick(attach)
  })

  // Streaming end may not change markdown text — re-attach when the flag flips.
  watch(
    () => options.isStreaming?.() === true,
    () => {
      nextTick(attach)
    }
  )

  onBeforeUnmount(() => {
    if (attachRaf) {
      window.cancelAnimationFrame(attachRaf)
      attachRaf = 0
    }
    bindDomObserver(null)
    for (const host of [...hosts.keys()]) destroyHost(host)
  })
}
