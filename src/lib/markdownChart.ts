/** Chart.js fence helpers for markdown (`chartjs` / `chart`). */

export const CHART_FENCE_LANGS = new Set(['chartjs', 'chart'])
export const MAX_CHART_JSON_BYTES = 100_000

export type ChartJsConfig = {
  type: string
  data: unknown
  options?: Record<string, unknown>
  /**
   * Host soft series palette (default). Set `false` only when the user
   * explicitly requested custom series colors — then model colors are kept.
   */
  pointerPalette?: boolean
  [key: string]: unknown
}

/** True unless root `pointerPalette: false` (user-requested custom colors). */
export function wantsHostSeriesPalette(config: ChartJsConfig): boolean {
  return config.pointerPalette !== false
}

export function isChartFenceLang(lang: string): boolean {
  return CHART_FENCE_LANGS.has(lang.toLowerCase())
}

export function tryParseChartConfig(
  raw: string
): { ok: true; config: ChartJsConfig; json: string } | { ok: false; reason: string } {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: false, reason: 'empty' }
  if (new TextEncoder().encode(trimmed).length > MAX_CHART_JSON_BYTES) {
    return { ok: false, reason: 'too_large' }
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(trimmed)
  } catch {
    return { ok: false, reason: 'invalid_json' }
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { ok: false, reason: 'not_object' }
  }
  const obj = parsed as Record<string, unknown>
  if (typeof obj.type !== 'string' || !obj.type.trim()) {
    return { ok: false, reason: 'missing_type' }
  }
  if (obj.data === undefined || obj.data === null) {
    return { ok: false, reason: 'missing_data' }
  }
  const json = JSON.stringify(parsed)
  return { ok: true, config: parsed as ChartJsConfig, json }
}

/** Encode JSON for a data-* attribute (base64url, UTF-8). */
export function encodeChartConfigAttr(json: string): string {
  const bytes = new TextEncoder().encode(json)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '')
}

export function decodeChartConfigAttr(encoded: string): string | null {
  try {
    const b64 = encoded.replace(/-/g, '+').replace(/_/g, '/')
    const pad = b64.length % 4 === 0 ? '' : '='.repeat(4 - (b64.length % 4))
    const binary = atob(b64 + pad)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    return new TextDecoder().decode(bytes)
  } catch {
    return null
  }
}

function cssHslChannels(varName: string, fallbackChannels: string): string {
  if (typeof document === 'undefined') return fallbackChannels
  const raw = getComputedStyle(document.documentElement).getPropertyValue(varName).trim()
  return raw || fallbackChannels
}

/**
 * Canvas 2D (esp. WKWebView / Tauri macOS) often rejects modern `hsl(H S% L%)`
 * space syntax. Prefer classic `hsl(H, S%, L%)` / `hsla(...)`.
 */
export function canvasHsl(varName: string, fallbackChannels: string, alpha = 1): string {
  const channels = cssHslChannels(varName, fallbackChannels)
  const parts = channels
    .replace(/[,/]/g, ' ')
    .trim()
    .split(/\s+/)
    .filter(Boolean)
  const h = parts[0] ?? '0'
  const s = (parts[1] ?? '0').includes('%') ? parts[1]! : `${parts[1] ?? '0'}%`
  const l = (parts[2] ?? '0').includes('%') ? parts[2]! : `${parts[2] ?? '0'}%`
  if (alpha < 1) return `hsla(${h}, ${s}, ${l}, ${alpha})`
  return `hsl(${h}, ${s}, ${l})`
}

/**
 * Resolve CSS color vars to `rgb()` / `rgba()` for WKWebView canvas.
 * Some WebKit builds mishandle hsl() in Chart.js stroke/fill paths.
 */
export function canvasRgb(varName: string, fallbackChannels: string, alpha = 1): string {
  const hsl = canvasHsl(varName, fallbackChannels, 1)
  if (typeof document === 'undefined') {
    return alpha < 1 ? canvasHsl(varName, fallbackChannels, alpha) : hsl
  }
  const probe = document.createElement('span')
  probe.style.color = hsl
  probe.style.display = 'none'
  document.body.appendChild(probe)
  const resolved = getComputedStyle(probe).color || hsl
  probe.remove()
  if (alpha < 1) {
    const m = resolved.match(/rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i)
    if (m) return `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${alpha})`
  }
  return resolved
}

export type ChartThemeColors = {
  foreground: string
  muted: string
  border: string
  card: string
  accent: string
  grid: string
}

/**
 * Soft series palette (dashboard-style): cool blue primary + warm peach contrast,
 * then muted teal / sand / sage / slate / mauve / olive.
 * Hex only — reliable on WKWebView canvas. Cycles by index when series exceed length.
 */
export const CHART_SERIES_PALETTE = [
  {
    border: '#4C8DDA',
    background: 'rgba(76, 141, 218, 0.78)',
    fill: 'rgba(76, 141, 218, 0.14)',
    point: '#4C8DDA',
  },
  {
    border: '#E8A07A',
    background: 'rgba(232, 160, 122, 0.82)',
    fill: 'rgba(232, 160, 122, 0.14)',
    point: '#E08A5E',
  },
  {
    border: '#5DADE2',
    background: 'rgba(93, 173, 226, 0.78)',
    fill: 'rgba(93, 173, 226, 0.14)',
    point: '#5DADE2',
  },
  {
    border: '#C4A574',
    background: 'rgba(196, 165, 116, 0.78)',
    fill: 'rgba(196, 165, 116, 0.14)',
    point: '#C4A574',
  },
  {
    border: '#7FBF9E',
    background: 'rgba(127, 191, 158, 0.78)',
    fill: 'rgba(127, 191, 158, 0.14)',
    point: '#7FBF9E',
  },
  {
    border: '#A8B2C1',
    background: 'rgba(168, 178, 193, 0.78)',
    fill: 'rgba(168, 178, 193, 0.14)',
    point: '#A8B2C1',
  },
  {
    border: '#C995A8',
    background: 'rgba(201, 149, 168, 0.78)',
    fill: 'rgba(201, 149, 168, 0.14)',
    point: '#C995A8',
  },
  {
    border: '#A3B07A',
    background: 'rgba(163, 176, 122, 0.78)',
    fill: 'rgba(163, 176, 122, 0.14)',
    point: '#A3B07A',
  },
] as const

/** Soft slice colors for pie / doughnut (aligned with series borders). */
export const CHART_SLICE_PALETTE = [
  '#4C8DDA',
  '#E8A07A',
  '#5DADE2',
  '#C4A574',
  '#7FBF9E',
  '#A8B2C1',
  '#C995A8',
  '#A3B07A',
] as const

export function readChartThemeColors(): ChartThemeColors {
  return {
    foreground: canvasRgb('--foreground', '222 47% 11%'),
    muted: canvasRgb('--muted', '215 16% 47%'),
    border: canvasRgb('--border', '220 13% 91%'),
    card: canvasRgb('--card', '0 0% 100%'),
    accent: canvasRgb('--accent', '250 42% 52%'),
    grid: canvasRgb('--border', '220 13% 91%', 0.45),
  }
}

/**
 * Export a PNG that matches on-screen chrome: card background under the
 * Chart.js bitmap (canvas alone is transparent and looks wrong in Preview).
 */
export function exportChartCanvasPngDataUrl(
  source: HTMLCanvasElement,
  opts?: { background?: string; pixelRatio?: number }
): string {
  const bg = opts?.background ?? readChartThemeColors().card
  const cssW = Math.max(1, Math.round(source.clientWidth || source.width))
  const cssH = Math.max(1, Math.round(source.clientHeight || source.height))
  const ratio = Math.max(
    1,
    opts?.pixelRatio ??
      (typeof window !== 'undefined' ? Math.min(window.devicePixelRatio || 1, 2) : 1)
  )
  const out = document.createElement('canvas')
  out.width = Math.max(1, Math.round(cssW * ratio))
  out.height = Math.max(1, Math.round(cssH * ratio))
  const ctx = out.getContext('2d')
  if (!ctx) {
    console.warn('[markdownChart] export: 2d context unavailable, falling back to source')
    return source.toDataURL('image/png')
  }
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0)
  ctx.fillStyle = bg
  ctx.fillRect(0, 0, cssW, cssH)
  // Draw the Chart.js bitmap scaled to CSS box so export matches on-screen size.
  ctx.drawImage(source, 0, 0, cssW, cssH)
  return out.toDataURL('image/png')
}

function applySeriesPalette(config: ChartJsConfig): ChartJsConfig {
  if (!isPlainObject(config.data) || !Array.isArray(config.data.datasets)) return config
  const type = String(config.type || '').toLowerCase()
  const isPie = type === 'pie' || type === 'doughnut'

  const datasets = config.data.datasets.map((ds, index) => {
    if (!isPlainObject(ds)) return ds
    if (isPie) {
      const n = Array.isArray(ds.data) ? ds.data.length : CHART_SLICE_PALETTE.length
      const slices = Array.from(
        { length: Math.max(n, 1) },
        (_, i) => CHART_SLICE_PALETTE[i % CHART_SLICE_PALETTE.length]!
      )
      return {
        ...ds,
        backgroundColor: slices,
        borderColor: '#ffffff',
        borderWidth: typeof ds.borderWidth === 'number' ? ds.borderWidth : 2,
      }
    }

    const swatch = CHART_SERIES_PALETTE[index % CHART_SERIES_PALETTE.length]!
    const dsType = String(ds.type || type || 'bar').toLowerCase()
    const isLine = dsType === 'line'
    const fillColor = isLine ? swatch.fill : swatch.background
    // Object-form fill (`above`/`below`) paints from those keys, not
    // `backgroundColor` — rewrite so model neon areas match the host palette.
    let fill = ds.fill
    if (isPlainObject(fill)) {
      const nextFill: Record<string, unknown> = { ...fill }
      if (typeof nextFill.above === 'string') nextFill.above = fillColor
      if (typeof nextFill.below === 'string') nextFill.below = fillColor
      fill = nextFill
    }
    return {
      ...ds,
      borderColor: swatch.border,
      backgroundColor: fillColor,
      pointBackgroundColor: swatch.point,
      pointBorderColor: swatch.border,
      borderWidth: typeof ds.borderWidth === 'number' ? ds.borderWidth : isLine ? 2 : 1,
      ...(fill !== ds.fill ? { fill } : {}),
    }
  })

  return {
    ...config,
    data: {
      ...config.data,
      datasets,
    },
  }
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v)
}

function coerceChartNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string') {
    const trimmed = value.trim().replace(/,/g, '')
    // Strip common unit suffixes the model may embed in data cells.
    const bare = trimmed
      .replace(/万人?/g, '')
      .replace(/亿人?/g, '')
      .replace(/[‰%]/g, '')
      .replace(/亿/g, '')
      .replace(/万/g, '')
      .trim()
    if (!bare) return null
    // parseFloat: more reliable than Number() on some WebKit builds (Chart.js#11389).
    const n = Number.parseFloat(bare)
    return Number.isFinite(n) ? n : null
  }
  return null
}

/**
 * Flatten every dataset to a primitive `number|null` array.
 * Object/`parsing:false` forms have been unreliable in macOS WKWebView.
 */
function coerceDataPoint(value: unknown): number | null {
  if (value == null) return null
  if (typeof value === 'number' || typeof value === 'string') {
    return coerceChartNumber(value)
  }
  if (Array.isArray(value) && value.length >= 2) {
    return coerceChartNumber(value[1])
  }
  if (isPlainObject(value)) {
    return coerceChartNumber(value.y ?? value.Y ?? value.x ?? value.X)
  }
  return null
}

function chartIndexAxis(config: ChartJsConfig): 'x' | 'y' {
  const opts = isPlainObject(config.options) ? config.options : {}
  return opts.indexAxis === 'y' ? 'y' : 'x'
}

function categoryLabelLength(label: unknown): number {
  return Array.from(String(label ?? '')).length
}

/**
 * Vertical bars + long Chinese category labels collapse the plot (labels eat
 * the 360px height). Auto-flip to horizontal unless the model set indexAxis.
 */
function preferHorizontalBarForLongLabels(config: ChartJsConfig): ChartJsConfig {
  if (String(config.type || '').toLowerCase() !== 'bar') return config
  const opts = isPlainObject(config.options) ? { ...config.options } : {}
  if (opts.indexAxis === 'x' || opts.indexAxis === 'y') return config
  const labels =
    isPlainObject(config.data) && Array.isArray(config.data.labels) ? config.data.labels : []
  if (labels.length < 2) return config
  const maxLen = Math.max(0, ...labels.map(categoryLabelLength))
  // Long names (报销口径) or many mid-length labels → horizontal.
  if (!(maxLen >= 8 || (labels.length >= 4 && maxLen >= 6))) return config

  opts.indexAxis = 'y'
  // Model often puts beginAtZero on y for vertical bars; after flip the value
  // axis is x — move the flag so the scale still starts at zero.
  if (isPlainObject(opts.scales)) {
    const scales = { ...opts.scales }
    const yScale = isPlainObject(scales.y) ? { ...scales.y } : null
    const xScale = isPlainObject(scales.x) ? { ...scales.x } : {}
    if (yScale && yScale.beginAtZero === true && xScale.beginAtZero == null) {
      xScale.beginAtZero = true
      delete yScale.beginAtZero
      scales.x = xScale
      scales.y = yScale
      opts.scales = scales
    }
  }
  return { ...config, options: opts }
}

/** Ensure scale numeric fields are real numbers (not numeric strings). */
function forceNumericScaleBounds(scale: Record<string, unknown>): Record<string, unknown> {
  const next = { ...scale }
  for (const key of ['min', 'max', 'suggestedMin', 'suggestedMax', 'grace']) {
    if (key === 'grace' && typeof next[key] === 'string') continue
    if (key in next) {
      const n = coerceChartNumber(next[key])
      if (n == null) delete next[key]
      else next[key] = n
    }
  }
  if (isPlainObject(next.ticks)) {
    const ticks = { ...next.ticks }
    if ('stepSize' in ticks) {
      const step = coerceChartNumber(ticks.stepSize)
      if (step == null) delete ticks.stepSize
      else ticks.stepSize = step
    }
    next.ticks = ticks
  }
  return next
}

/** Drop JSON-only leftovers that break Chart.js scales/ticks (string "callbacks", etc.). */
function sanitizeScale(scale: Record<string, unknown>): Record<string, unknown> {
  const next = { ...scale }
  for (const key of ['callback', 'afterBuildTicks', 'beforeBuildTicks', 'afterFit']) {
    if (key in next && typeof next[key] !== 'function') delete next[key]
  }
  if (isPlainObject(next.ticks)) {
    const ticks = { ...next.ticks }
    for (const key of ['callback', 'major', 'format']) {
      if (key in ticks && typeof ticks[key] !== 'function' && typeof ticks[key] !== 'object') {
        delete ticks[key]
      }
    }
    if (typeof ticks.callback === 'string') delete ticks.callback
    // Force ticks visible unless the model explicitly hid them.
    if (ticks.display === false) {
      /* keep explicit hide */
    } else {
      ticks.display = true
    }
    next.ticks = ticks
  }
  // Absurd fixed ranges crush series to the axis — drop when clearly wrong.
  const min = coerceChartNumber(next.min)
  const max = coerceChartNumber(next.max)
  if (min != null && max != null && max <= min) {
    delete next.min
    delete next.max
  }
  return next
}

function looksLikeJsCallback(value: unknown): boolean {
  return typeof value === 'string' && /=>|function\s*\(/.test(value)
}

/**
 * Model JSON cannot carry real functions. String "callbacks" in `segment` /
 * scriptable colors crash Chart.js when invoked — strip them.
 */
function sanitizeDatasetScriptables(ds: Record<string, unknown>): Record<string, unknown> {
  const next = { ...ds }
  if (isPlainObject(next.segment)) {
    const seg = { ...next.segment }
    for (const key of Object.keys(seg)) {
      if (typeof seg[key] !== 'function') delete seg[key]
    }
    if (Object.keys(seg).length) next.segment = seg
    else delete next.segment
  }
  for (const key of [
    'borderColor',
    'backgroundColor',
    'pointBackgroundColor',
    'pointBorderColor',
    'borderWidth',
    'pointRadius',
  ]) {
    if (looksLikeJsCallback(next[key])) delete next[key]
  }
  return next
}

/**
 * Host-side negative segment styling (real functions — safe for Chart.js).
 * Keeps the series soft color; only dashes segments that sit below zero.
 */
function withNegativeSegmentStyle(
  ds: Record<string, unknown>,
  values: Array<number | null>
): Record<string, unknown> {
  const hasNeg = values.some(v => typeof v === 'number' && v < 0)
  const hasPos = values.some(v => typeof v === 'number' && v >= 0)
  if (!hasNeg || !hasPos) return ds
  // Keep any real function segment the host already attached; otherwise add ours.
  if (isPlainObject(ds.segment)) {
    const seg = ds.segment
    if (Object.values(seg).some(v => typeof v === 'function')) return ds
  }
  return {
    ...ds,
    segment: {
      borderDash: (ctx: { p0?: { parsed?: { y?: number | null } } }) => {
        const y0 = ctx.p0?.parsed?.y
        return typeof y0 === 'number' && y0 < 0 ? [6, 4] : undefined
      },
    },
  }
}

function pushNumericPoint(out: number[], point: unknown) {
  const n = coerceDataPoint(point)
  if (n != null) out.push(n)
}

function datasetAxisId(ds: Record<string, unknown>): string {
  const id = ds.yAxisID
  return typeof id === 'string' && id.trim() ? id.trim() : 'y'
}

function collectNumericValues(data: unknown): number[] {
  const out: number[] = []
  if (!isPlainObject(data)) return out
  const datasets = data.datasets
  if (!Array.isArray(datasets)) return out
  for (const ds of datasets) {
    if (!isPlainObject(ds) || !Array.isArray(ds.data)) continue
    for (const point of ds.data) pushNumericPoint(out, point)
  }
  return out
}

/** Values for one value-axis (honours dataset `yAxisID`; default `y`). */
function collectNumericValuesForAxis(data: unknown, axisId: string): number[] {
  const out: number[] = []
  if (!isPlainObject(data)) return out
  const datasets = data.datasets
  if (!Array.isArray(datasets)) return out
  for (const ds of datasets) {
    if (!isPlainObject(ds) || !Array.isArray(ds.data)) continue
    if (datasetAxisId(ds) !== axisId) continue
    for (const point of ds.data) pushNumericPoint(out, point)
  }
  return out
}

function isValueAxisKey(key: string): boolean {
  return key === 'y' || key === 'r' || /^y\d+$/i.test(key)
}

/**
 * Drop min/max (and suggested*) when they leave most of the plot empty.
 * Typical model bug: max=15 / 3000 while series sits near the floor.
 */
function tightenScaleToData(
  scale: Record<string, unknown>,
  dataMin: number,
  dataMax: number
): Record<string, unknown> {
  const next = { ...scale }
  const fixedMin = coerceChartNumber(next.min)
  const fixedMax = coerceChartNumber(next.max)
  const suggestedMin = coerceChartNumber(next.suggestedMin)
  const suggestedMax = coerceChartNumber(next.suggestedMax)
  const hi = fixedMax ?? suggestedMax
  const lo = fixedMin ?? suggestedMin

  if (hi != null) {
    const rangeLo = lo ?? Math.min(dataMin, dataMax >= 0 ? 0 : dataMin)
    const range = hi - rangeLo
    // >35% empty headroom above the series → let Chart.js autoscale.
    if (range > 0 && (hi - dataMax) / range > 0.35) {
      delete next.max
      delete next.suggestedMax
    }
  }

  const hiAfter = coerceChartNumber(next.max) ?? coerceChartNumber(next.suggestedMax) ?? dataMax
  const loBound = fixedMin ?? suggestedMin
  if (loBound != null) {
    const range = hiAfter - loBound
    const beginAtZero = next.beginAtZero === true && loBound === 0 && dataMin >= 0
    if (!beginAtZero && range > 0 && (dataMin - loBound) / range > 0.35) {
      delete next.min
      delete next.suggestedMin
    }
  }

  const min = coerceChartNumber(next.min)
  const max = coerceChartNumber(next.max)
  if (min != null && max != null && max <= min) {
    delete next.min
    delete next.max
  }
  return next
}

function sanitizeScalesForData(
  scales: Record<string, unknown>,
  data: unknown
): Record<string, unknown> {
  const next = { ...scales }
  for (const key of Object.keys(next)) {
    const scale = next[key]
    if (!isPlainObject(scale)) continue
    let cleaned = forceNumericScaleBounds(sanitizeScale(scale))
    if (isValueAxisKey(key)) {
      const values = collectNumericValuesForAxis(data, key)
      if (values.length) {
        cleaned = tightenScaleToData(cleaned, Math.min(...values), Math.max(...values))
      }
    }
    next[key] = forceNumericScaleBounds(cleaned)
  }
  return next
}

function forceCartesianScaleTypes(
  scales: Record<string, unknown>,
  indexAxis: 'x' | 'y'
): Record<string, unknown> {
  const next = { ...scales }
  const categoryKey = indexAxis === 'y' ? 'y' : 'x'
  const primaryValueKey = indexAxis === 'y' ? 'x' : 'y'

  const cat = isPlainObject(next[categoryKey]) ? { ...next[categoryKey] } : {}
  cat.type = 'category'
  next[categoryKey] = cat

  const primary = isPlainObject(next[primaryValueKey]) ? { ...next[primaryValueKey] } : {}
  if (primary.type == null || primary.type === 'category') primary.type = 'linear'
  next[primaryValueKey] = primary

  for (const key of Object.keys(next)) {
    if (key === categoryKey) continue
    if (!(isValueAxisKey(key) || key === primaryValueKey)) continue
    const scale = isPlainObject(next[key]) ? { ...next[key] } : {}
    if (scale.type == null || scale.type === 'category') scale.type = 'linear'
    next[key] = scale
  }

  return next
}

/** Normalize model JSON so Chart.js can scale/draw reliably. */
export function sanitizeChartConfig(config: ChartJsConfig): ChartJsConfig {
  let cloned: ChartJsConfig
  try {
    cloned = JSON.parse(JSON.stringify(config)) as ChartJsConfig
  } catch {
    return config
  }

  const useHostPalette = wantsHostSeriesPalette(cloned)
  // Host-only flag — not a Chart.js field.
  delete cloned.pointerPalette

  cloned = preferHorizontalBarForLongLabels(cloned)

  if (isPlainObject(cloned.data) && Array.isArray(cloned.data.datasets)) {
    cloned.data = {
      ...cloned.data,
      datasets: cloned.data.datasets.map(ds => {
        if (!isPlainObject(ds)) return ds
        if (!Array.isArray(ds.data)) return ds
        const values = ds.data.map(coerceDataPoint)
        // WKWebView-safe line defaults: avoid cubic tension path bugs.
        let next: Record<string, unknown> = sanitizeDatasetScriptables({
          ...ds,
          data: values,
        })
        if (typeof next.tension === 'number' && next.tension > 0) next.tension = 0
        // Negative-segment host colors use the soft palette — skip when custom.
        if (useHostPalette) next = withNegativeSegmentStyle(next, values)
        return next
      }),
    }
  }

  const indexAxis = chartIndexAxis(cloned)
  if (isPlainObject(cloned.options)) {
    const opts = { ...cloned.options }
    if (typeof opts.animation === 'string') delete opts.animation
    // chartjs-plugin-annotation is not bundled — drop model annotation blocks.
    if (isPlainObject(opts.plugins)) {
      const plugins = { ...opts.plugins }
      delete plugins.annotation
      opts.plugins = plugins
    }
    if (isPlainObject(opts.scales)) {
      let scales = sanitizeScalesForData({ ...opts.scales }, cloned.data)
      if (isCartesianType(String(cloned.type || '')) && String(cloned.type).toLowerCase() !== 'radar') {
        scales = forceCartesianScaleTypes(scales, indexAxis)
      }
      opts.scales = scales
    }
    // Default Chart.js parsing over primitive arrays — most reliable on WKWebView.
    delete opts.parsing
    if (
      isCartesianType(String(cloned.type || '')) &&
      String(cloned.type).toLowerCase() !== 'radar' &&
      !isPlainObject(opts.scales)
    ) {
      opts.scales = forceCartesianScaleTypes({}, indexAxis)
    }
    cloned.options = opts
  } else if (isCartesianType(String(cloned.type || '')) && String(cloned.type).toLowerCase() !== 'radar') {
    cloned.options = {
      scales: forceCartesianScaleTypes({}, indexAxis),
    }
  }

  return cloned
}

function isCartesianType(type: string): boolean {
  return ['bar', 'line', 'scatter', 'bubble', 'radar'].includes(type.toLowerCase())
}

/** Deep-merge theme defaults under user options (user wins on conflict). */
export function applyChartTheme(config: ChartJsConfig): ChartJsConfig {
  const useHostPalette = wantsHostSeriesPalette(config)
  const sanitizedBase = sanitizeChartConfig(config)
  const sanitized = useHostPalette ? applySeriesPalette(sanitizedBase) : sanitizedBase
  const theme = readChartThemeColors()
  const userOptions = isPlainObject(sanitized.options) ? sanitized.options : {}
  const plugins = isPlainObject(userOptions.plugins) ? userOptions.plugins : {}
  const legend = isPlainObject(plugins.legend) ? plugins.legend : {}
  const legendLabels = isPlainObject(legend.labels) ? legend.labels : {}
  const title = isPlainObject(plugins.title) ? plugins.title : {}
  const subtitle = isPlainObject(plugins.subtitle) ? plugins.subtitle : {}

  let scales: Record<string, unknown> = isPlainObject(userOptions.scales)
    ? { ...userOptions.scales }
    : {}

  // Ensure cartesian charts have themed x/y ticks even when the model omitted scales.
  if (isCartesianType(sanitized.type) && sanitized.type.toLowerCase() !== 'radar') {
    for (const key of ['x', 'y']) {
      if (!isPlainObject(scales[key])) scales[key] = {}
    }
  }

  for (const key of Object.keys(scales)) {
    const scale = scales[key]
    if (!isPlainObject(scale)) continue
    const cleaned = sanitizeScale(scale)
    const ticks = isPlainObject(cleaned.ticks) ? cleaned.ticks : {}
    const grid = isPlainObject(cleaned.grid) ? cleaned.grid : {}
    const scaleTitle = isPlainObject(cleaned.title) ? cleaned.title : {}
    const tickColor =
      typeof ticks.color === 'string' && ticks.color.includes('hsl(') && !ticks.color.includes(',')
        ? theme.muted
        : typeof ticks.color === 'string'
          ? ticks.color
          : theme.muted
    scales[key] = {
      ...cleaned,
      ticks: {
        display: ticks.display === false ? false : true,
        font: { size: 11 },
        ...ticks,
        color: tickColor,
      },
      grid: {
        color: theme.grid,
        ...grid,
      },
      title: {
        color: theme.muted,
        ...scaleTitle,
      },
    }
  }

  scales = sanitizeScalesForData(scales, sanitized.data)
  if (isCartesianType(sanitized.type) && sanitized.type.toLowerCase() !== 'radar') {
    scales = forceCartesianScaleTypes(scales, chartIndexAxis(sanitized))
  }

  // Host sets a fixed canvas height — never let model aspectRatio flatten the plot.
  const { aspectRatio: _ignoredAspectRatio, parsing: _ignoredParsing, ...userOptionsRest } =
    userOptions
  const tooltipIn = isPlainObject(plugins.tooltip) ? plugins.tooltip : {}
  // Drop non-function tooltip callbacks from model JSON (they break Chart.js).
  const { callbacks: tooltipCallbacks, ...tooltipRest } = tooltipIn
  const safeTooltipCallbacks =
    tooltipCallbacks &&
    typeof tooltipCallbacks === 'object' &&
    !Array.isArray(tooltipCallbacks) &&
    Object.values(tooltipCallbacks).every(v => typeof v === 'function')
      ? tooltipCallbacks
      : undefined
  const userInteraction = isPlainObject(userOptions.interaction) ? userOptions.interaction : {}
  const dpr =
    typeof window !== 'undefined' && Number.isFinite(window.devicePixelRatio)
      ? Math.min(Math.max(window.devicePixelRatio, 1), 2)
      : 1

  return {
    ...sanitized,
    options: {
      responsive: true,
      ...userOptionsRest,
      // Disable animations — WKWebView has left charts stuck on the wrong first paint.
      animation: false,
      animations: false,
      devicePixelRatio: dpr,
      maintainAspectRatio: false,
      // Near-vertical hover for time series (no need to hit the exact point).
      interaction: {
        mode: 'index',
        intersect: false,
        ...userInteraction,
      },
      layout: {
        padding: { top: 4, right: 8, bottom: 0, left: 4 },
        ...(isPlainObject(userOptions.layout) ? userOptions.layout : {}),
      },
      plugins: {
        ...plugins,
        legend: {
          position: 'top',
          ...legend,
          labels: { color: theme.foreground, boxWidth: 12, ...legendLabels },
        },
        title: {
          color: theme.foreground,
          ...title,
        },
        subtitle: {
          color: theme.muted,
          ...subtitle,
        },
        tooltip: {
          enabled: true,
          mode: 'index',
          intersect: false,
          backgroundColor: canvasRgb('--foreground', '222 47% 11%', 0.92),
          titleColor: canvasRgb('--card', '0 0% 100%'),
          bodyColor: canvasRgb('--card', '0 0% 100%'),
          borderColor: theme.border,
          borderWidth: 1,
          padding: 10,
          displayColors: true,
          ...tooltipRest,
          ...(safeTooltipCallbacks ? { callbacks: safeTooltipCallbacks } : {}),
        },
      },
      scales: Object.keys(scales).length ? scales : userOptions.scales,
    },
  }
}
