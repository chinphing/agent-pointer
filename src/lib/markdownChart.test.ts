import { describe, expect, it } from 'vitest'
import {
  applyChartTheme,
  canvasHsl,
  CHART_SERIES_PALETTE,
  decodeChartConfigAttr,
  encodeChartConfigAttr,
  exportChartCanvasPngDataUrl,
  isChartFenceLang,
  sanitizeChartConfig,
  tryParseChartConfig,
} from './markdownChart'
import {
  parseMarkdown,
  stabilizeStreamingChartFences,
  STREAMING_CHART_HOST_HTML,
  STREAMING_CHART_STUB_JSON,
} from './markdownConfig'

describe('markdownChart', () => {
  it('recognizes chart fence langs', () => {
    expect(isChartFenceLang('chartjs')).toBe(true)
    expect(isChartFenceLang('chart')).toBe(true)
    expect(isChartFenceLang('bash')).toBe(false)
  })

  it('parses a valid Chart.js config', () => {
    const raw = JSON.stringify({
      type: 'bar',
      data: { labels: ['A'], datasets: [{ data: [1] }] },
      options: { indexAxis: 'y' },
    })
    const parsed = tryParseChartConfig(raw)
    expect(parsed.ok).toBe(true)
    if (parsed.ok) {
      expect(parsed.config.type).toBe('bar')
    }
  })

  it('flips long Chinese category labels to horizontal bars', () => {
    const sanitized = sanitizeChartConfig({
      type: 'bar',
      data: {
        labels: [
          '已匹配（✅）',
          '已剔除（⛔）',
          '已匹配+已剔除',
          '凭证总额',
          '未匹配发票的凭证小计',
          '未匹配凭证的发票小计',
        ],
        datasets: [{ label: '金额（元）', data: [2496, 960, 3456, 4686, 1230, 0] }],
      },
      options: {
        plugins: { legend: { display: false } },
        scales: { y: { beginAtZero: true } },
      },
    })
    expect((sanitized.options as { indexAxis?: string }).indexAxis).toBe('y')
    const scales = (sanitized.options as { scales?: Record<string, Record<string, unknown>> }).scales
    expect(scales?.x?.beginAtZero).toBe(true)
    expect(scales?.y?.beginAtZero).toBeUndefined()
  })

  it('keeps short-label vertical bars', () => {
    const sanitized = sanitizeChartConfig({
      type: 'bar',
      data: {
        labels: ['A', 'B', 'C'],
        datasets: [{ data: [1, 2, 3] }],
      },
    })
    expect((sanitized.options as { indexAxis?: string } | undefined)?.indexAxis).toBeUndefined()
  })

  it('rejects missing type/data and invalid json', () => {
    expect(tryParseChartConfig('{').ok).toBe(false)
    expect(tryParseChartConfig('{"data":{}}').ok).toBe(false)
    expect(tryParseChartConfig('{"type":"bar"}').ok).toBe(false)
  })

  it('round-trips attribute encoding', () => {
    const json = '{"type":"pie","data":{"labels":["x"],"datasets":[{"data":[1]}]}}'
    const encoded = encodeChartConfigAttr(json)
    expect(encoded.includes('+')).toBe(false)
    expect(decodeChartConfigAttr(encoded)).toBe(json)
  })

  it('emits classic comma hsl for canvas', () => {
    const color = canvasHsl('--muted', '240 4% 42%')
    expect(color.startsWith('hsl(') || color.startsWith('hsla(')).toBe(true)
    expect(color.includes(',')).toBe(true)
  })

  it('exportChartCanvasPngDataUrl returns a png data url', () => {
    // Node has no real canvas 2d — only assert the helper fails soft / returns a data url shape when possible.
    if (typeof document === 'undefined') return
    const canvas = document.createElement('canvas')
    canvas.width = 40
    canvas.height = 20
    Object.defineProperty(canvas, 'clientWidth', { value: 40 })
    Object.defineProperty(canvas, 'clientHeight', { value: 20 })
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.fillStyle = '#4C8DDA'
    ctx.fillRect(0, 0, 40, 20)
    const url = exportChartCanvasPngDataUrl(canvas, { background: '#ffffff', pixelRatio: 1 })
    expect(url.startsWith('data:image/png')).toBe(true)
  })

  it('coerces string data and drops absurd y max', () => {
    const sanitized = sanitizeChartConfig({
      type: 'line',
      data: {
        labels: ['a', 'b'],
        datasets: [{ data: ['5.5', '14.12'] }],
      },
      options: {
        scales: {
          y: { min: 0, max: 1_000_000_000, ticks: { callback: 'function(v){return v}' } },
        },
      },
    })
    const ds = (sanitized.data as { datasets: { data: unknown[] }[] }).datasets[0]!
    expect(ds.data).toEqual([5.5, 14.12])
    const y = (sanitized.options as { scales: { y: Record<string, unknown> } }).scales.y
    expect(y.max).toBeUndefined()
    expect(y.ticks && (y.ticks as { callback?: unknown }).callback).toBeUndefined()
    expect(y.type).toBe('linear')
    expect((sanitized.options as { parsing?: boolean }).parsing).toBeUndefined()
  })

  it('keeps primitive population data and 5–15 scale', () => {
    const sanitized = sanitizeChartConfig({
      type: 'line',
      data: {
        labels: ['1950', '2022', '2025'],
        datasets: [{ data: [5.52, 14.12, 14.05], fill: true, tension: 0.4 }],
      },
      options: {
        scales: { y: { min: 5, max: 15, ticks: { stepSize: 1, callback: "v => v + '亿'" } } },
      },
    })
    const ds = (sanitized.data as { datasets: { data: number[]; tension?: number }[] }).datasets[0]!
    expect(ds.data[1]).toBe(14.12)
    expect(ds.tension).toBe(0)
    const scales = (
      sanitized.options as { scales: { x: { type: string }; y: Record<string, unknown> } }
    ).scales
    expect(scales.x.type).toBe('category')
    expect(scales.y.type).toBe('linear')
    expect(scales.y.min).toBe(5)
    expect(scales.y.max).toBe(15)
  })

  it('drops loose y max that leaves series glued to the floor', () => {
    const sanitized = sanitizeChartConfig({
      type: 'line',
      data: {
        labels: ['1950', '2022'],
        datasets: [{ data: [5.5, 5.8] }],
      },
      options: {
        scales: {
          y: { min: 5, max: 15 },
        },
      },
    })
    const y = (sanitized.options as { scales: { y: Record<string, unknown> } }).scales.y
    expect(y.max).toBeUndefined()
    // min=5 also becomes loose once max is cleared against a flat ~5.5 series
    expect(y.min).toBeUndefined()
  })

  it('drops oversized bar max while keeping beginAtZero', () => {
    const sanitized = sanitizeChartConfig({
      type: 'bar',
      data: {
        labels: ['2016', '2020'],
        datasets: [
          { label: 'birth', data: [180, 120] },
          { label: 'death', data: [90, 100] },
        ],
      },
      options: {
        scales: {
          y: { beginAtZero: true, min: 0, max: 3000, title: { display: true, text: '万人' } },
        },
      },
    })
    const y = (sanitized.options as { scales: { y: Record<string, unknown> } }).scales.y
    expect(y.max).toBeUndefined()
    expect(y.min).toBe(0)
  })

  it('keeps a tight max that still fits the series', () => {
    const sanitized = sanitizeChartConfig({
      type: 'line',
      data: {
        labels: ['a', 'b'],
        datasets: [{ data: [13.8, 14.12] }],
      },
      options: {
        scales: { y: { min: 5, max: 15 } },
      },
    })
    const y = (sanitized.options as { scales: { y: Record<string, unknown> } }).scales.y
    expect(y.max).toBe(15)
  })

  it('forces maintainAspectRatio false even if model sets aspectRatio', () => {
    const themed = applyChartTheme({
      type: 'bar',
      data: { labels: ['A'], datasets: [{ data: [3] }] },
      options: { aspectRatio: 4, maintainAspectRatio: true },
    })
    const opts = themed.options as { maintainAspectRatio: boolean; aspectRatio?: number }
    expect(opts.maintainAspectRatio).toBe(false)
    expect(opts.aspectRatio).toBeUndefined()
  })

  it('applies soft series palette and overrides neon model colors', () => {
    const themed = applyChartTheme({
      type: 'bar',
      data: {
        labels: ['A', 'B'],
        datasets: [
          { data: [1, 2], backgroundColor: '#e63946', borderColor: '#ff0000' },
          { type: 'line', data: [3, 4], borderColor: '#00ff00' },
        ],
      },
    })
    const datasets = (themed.data as { datasets: Record<string, unknown>[] }).datasets
    expect(datasets[0]!.borderColor).toBe('#4C8DDA')
    expect(String(datasets[0]!.backgroundColor)).toContain('76, 141, 218')
    expect(datasets[1]!.borderColor).toBe('#E8A07A')
    expect(String(datasets[1]!.backgroundColor)).toContain('232, 160, 122')
  })

  it('cycles soft palette for a 5th series and exposes eight swatches', () => {
    expect(CHART_SERIES_PALETTE).toHaveLength(8)
    const themed = applyChartTheme({
      type: 'line',
      data: {
        labels: ['a'],
        datasets: Array.from({ length: 5 }, () => ({ data: [1] })),
      },
    })
    const datasets = (themed.data as { datasets: Record<string, unknown>[] }).datasets
    expect(datasets[4]!.borderColor).toBe('#7FBF9E')
  })

  it('keeps model colors when pointerPalette is false', () => {
    const themed = applyChartTheme({
      type: 'line',
      pointerPalette: false,
      data: {
        labels: ['a', 'b'],
        datasets: [
          {
            data: [1, 2],
            borderColor: '#e63946',
            backgroundColor: 'rgba(230,57,70,0.12)',
            fill: { target: 'origin', above: 'rgba(230,57,70,0.15)' },
          },
        ],
      },
    })
    expect(themed.pointerPalette).toBeUndefined()
    const ds = (themed.data as { datasets: Record<string, unknown>[] }).datasets[0]!
    expect(ds.borderColor).toBe('#e63946')
    expect(ds.backgroundColor).toBe('rgba(230,57,70,0.12)')
    expect((ds.fill as { above: string }).above).toBe('rgba(230,57,70,0.15)')
  })

  it('rewrites object-form fill.above to host palette (not just backgroundColor)', () => {
    const themed = applyChartTheme({
      type: 'line',
      data: {
        labels: ['a', 'b'],
        datasets: [
          {
            data: [1, 2],
            fill: { target: 'origin', above: 'rgba(230,57,70,0.15)' },
          },
        ],
      },
    })
    const ds = (themed.data as { datasets: Record<string, unknown>[] }).datasets[0]!
    const fill = ds.fill as { above: string }
    expect(fill.above).toContain('76, 141, 218')
    expect(String(ds.backgroundColor)).toContain('76, 141, 218')
  })

  it('strips string segment/annotation callbacks that crash Chart.js', () => {
    const sanitized = sanitizeChartConfig({
      type: 'line',
      data: {
        labels: ['a', 'b', 'c'],
        datasets: [
          {
            data: [2, -1, 3],
            segment: {
              borderDash: "ctx => ctx.p0.parsed.y < 0 ? [6,4] : undefined",
              borderColor: "ctx => ctx.p0.parsed.y < 0 ? '#d00000' : '#8338ec'",
            },
          },
        ],
      },
      options: {
        plugins: {
          annotation: {
            annotations: { zeroLine: { type: 'line', yMin: 0, yMax: 0 } },
          },
        },
        scales: { y: { ticks: { callback: "v => v + '‰'" } } },
      },
    })
    const ds = (sanitized.data as { datasets: Record<string, unknown>[] }).datasets[0]!
    // String segment removed; host dashes negatives in the series soft color (no red).
    const seg = ds.segment as Record<string, unknown> | undefined
    expect(seg).toBeTruthy()
    expect(seg!.borderColor).toBeUndefined()
    expect(typeof seg!.borderDash).toBe('function')
    const plugins = (sanitized.options as { plugins: Record<string, unknown> }).plugins
    expect(plugins.annotation).toBeUndefined()
    const yTicks = (sanitized.options as { scales: { y: { ticks: Record<string, unknown> } } }).scales
      .y.ticks
    expect(yTicks.callback).toBeUndefined()
  })

  it('applyChartTheme keeps cartesian tick colors canvas-safe', () => {
    const themed = applyChartTheme({
      type: 'bar',
      data: { labels: ['A'], datasets: [{ data: [3] }] },
    })
    const y = (themed.options as { scales: { y: { ticks: { color: string; display: boolean } } } })
      .scales.y
    expect(y.ticks.display).toBe(true)
    // Browser: rgb(); node tests fall back to classic hsl(H, S%, L%).
    expect(y.ticks.color.includes(',')).toBe(true)
  })
})

describe('parseMarkdown chart fences', () => {
  it('emits an md-chart host for valid chartjs JSON', () => {
    const src =
      '```chartjs\n' +
      '{"type":"bar","data":{"labels":["A","B"],"datasets":[{"data":[1,2]}]}}\n' +
      '```'
    const html = parseMarkdown(src)
    expect(html).toContain('class="md-chart group"')
    expect(html).toContain('data-chart-config=')
    expect(html).toContain('md-chart-canvas-wrap')
    expect(html).toContain('md-chart-canvas-box')
    expect(html).not.toContain('code-block')
  })

  it('emits an invalid host for incomplete chart JSON', () => {
    const src = '```chart\n{"type":"bar"\n```'
    const html = parseMarkdown(src)
    expect(html).toContain('md-chart--invalid')
    expect(html).toContain('data-chart-config=')
  })

  it('keeps ordinary code fences as code-block', () => {
    const html = parseMarkdown('```bash\necho hi\n```')
    expect(html).toContain('code-block')
    expect(html).not.toContain('md-chart')
  })

  it('stabilizeStreamingChartFences collapses growing chart JSON to a stub', () => {
    const a = stabilizeStreamingChartFences(
      'Intro\n\n```chartjs\n{"type":"line","data":{"labels":["1"]\n'
    )
    const b = stabilizeStreamingChartFences(
      'Intro\n\n```chartjs\n{"type":"line","data":{"labels":["1","2"],"datasets":[{"data":[1,2]}]}}\n```\n'
    )
    expect(a).toContain(STREAMING_CHART_STUB_JSON)
    expect(b).toContain(STREAMING_CHART_STUB_JSON)
    // Trailing newline on the closed fence input may remain; bodies match.
    expect(a.replace(/\n+$/, '')).toBe(b.replace(/\n+$/, ''))
  })

  it('streamingCharts parse keeps identical HTML while fence JSON grows', () => {
    const prefix = '人口趋势如下：\n\n'
    const htmlA = parseMarkdown(
      prefix + '```chartjs\n{"type":"bar","data":{"labels":["A"]\n',
      { streamingCharts: true }
    )
    const htmlB = parseMarkdown(
      prefix +
        '```chartjs\n{"type":"bar","data":{"labels":["A","B"],"datasets":[{"data":[1,2]}]}}\n```\n',
      { streamingCharts: true }
    )
    expect(htmlA).toBe(htmlB)
    expect(htmlA).toContain(STREAMING_CHART_HOST_HTML.trim())
    expect(htmlA).toContain('图表生成中…')
  })
})
