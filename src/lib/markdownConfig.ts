import { marked } from 'marked'
import {
  encodeChartConfigAttr,
  isChartFenceLang,
  tryParseChartConfig,
} from './markdownChart'

// Configure marked once at module load — all importers share this instance.
marked.setOptions({ breaks: true, gfm: true })

/** Fixed JSON body for streaming chart placeholders (not a real chart). */
export const STREAMING_CHART_STUB_JSON = '{"pointerChartPending":true}'

/**
 * Stable markdown fence emitted while the assistant turn is still streaming.
 * Growing Chart.js JSON is discarded so parsed HTML stays identical across ticks.
 */
export const STREAMING_CHART_FENCE =
  '```chartjs\n' + STREAMING_CHART_STUB_JSON + '\n```'

/** Pre-built pending host — identical string every stream tick. */
export const STREAMING_CHART_HOST_HTML =
  `<div class="md-chart group md-chart--pending" data-chart-config="${encodeChartConfigAttr(STREAMING_CHART_STUB_JSON)}">` +
  `<div class="md-chart-toolbar" hidden></div>` +
  `<div class="md-chart-canvas-wrap"><div class="md-chart-status md-chart-status-pending">图表生成中…</div></div>` +
  `<pre class="md-chart-source" hidden></pre>` +
  `</div>\n`

/** Set only for the duration of `parseMarkdown(..., { streamingCharts: true })`. */
let parseStreamingCharts = false

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function tableAlignClass(align: string | null | undefined): string | null {
  if (align === 'right') return 'md-align-right'
  if (align === 'center') return 'md-align-center'
  if (align === 'left') return 'md-align-left'
  return null
}

function tableCellHtml(tag: 'th' | 'td', text: string, align: string | null | undefined): string {
  const cls = tableAlignClass(align)
  const attr = cls ? ` class="${cls}"` : ''
  return `<${tag}${attr}>` + marked.parseInline(text) + `</${tag}>`
}

function chartHostHtml(raw: string, valid: boolean): string {
  const encoded = encodeChartConfigAttr(raw.trim() || '{}')
  const stateClass = valid ? '' : ' md-chart--invalid'
  return (
    `<div class="md-chart group${stateClass}" data-chart-config="${encoded}">` +
    `<div class="md-chart-toolbar"></div>` +
    `<div class="md-chart-canvas-wrap"><div class="md-chart-canvas-box"><canvas role="img" aria-label="chart"></canvas></div></div>` +
    `<pre class="md-chart-source" hidden></pre>` +
    `</div>\n`
  )
}

const CHART_OPEN_RE = /^[ \t]*```(chartjs|chart)[ \t]*$/i
const FENCE_CLOSE_RE = /^[ \t]*```[ \t]*$/

/**
 * Replace open/closed `chartjs` / `chart` fences with a fixed stub fence so
 * streaming Markdown re-parses do not rewrite the chart card HTML every token.
 */
export function stabilizeStreamingChartFences(src: string): string {
  if (!src.includes('```')) return src
  const lines = src.split('\n')
  const out: string[] = []
  let i = 0
  let replaced = false
  while (i < lines.length) {
    const line = lines[i]!
    if (CHART_OPEN_RE.test(line)) {
      i += 1
      while (i < lines.length && !FENCE_CLOSE_RE.test(lines[i]!)) i += 1
      if (i < lines.length) i += 1
      out.push('```chartjs', STREAMING_CHART_STUB_JSON, '```')
      replaced = true
      continue
    }
    out.push(line)
    i += 1
  }
  return replaced ? out.join('\n') : src
}

marked.use({
  renderer: {
    table({ header, rows, align }) {
      const h = header
        .map((c, i) => tableCellHtml('th', c.text ?? '', c.align ?? align?.[i]))
        .join('')
      const body = rows
        .map(r => '<tr>' + r.map((c, i) => tableCellHtml('td', c.text ?? '', c.align ?? align?.[i])).join('') + '</tr>')
        .join('')
      return (
        '<div class="table-wrapper"><table><thead><tr>' +
        h +
        '</tr></thead><tbody>' +
        body +
        '</tbody></table></div>'
      )
    },
    code({ text, lang, escaped }) {
      const langString = (lang || '').match(/^\S*/)?.[0] || ''
      const code = text.replace(/\n$/, '') + '\n'
      if (isChartFenceLang(langString)) {
        if (parseStreamingCharts) return STREAMING_CHART_HOST_HTML
        const raw = text.replace(/\n$/, '')
        if (raw.trim() === STREAMING_CHART_STUB_JSON) return STREAMING_CHART_HOST_HTML
        const parsed = tryParseChartConfig(raw)
        if (parsed.ok) return chartHostHtml(parsed.json, true)
        // Incomplete stream or bad JSON: keep a host so the UI can show pending/error.
        return chartHostHtml(raw, false)
      }
      const body = escaped ? code : escapeHtml(code)
      const langClass = langString ? ` class="language-${escapeHtml(langString)}"` : ''
      return (
        `<div class="code-block"><pre><code${langClass}>${body}</code></pre></div>\n`
      )
    },
  },
})

export type ParseMarkdownOptions = {
  /**
   * While the assistant turn is streaming, collapse chart fences to a fixed
   * pending host so `v-html` does not flash on every token.
   */
  streamingCharts?: boolean
}

/**
 * Parse Markdown to HTML with shared configuration.
 *
 * Includes a pre-processing step that inserts a blank line after GFM tables
 * when the next line is not a pipe or whitespace, which prevents the parser
 * from swallowing the table into the following paragraph.
 */
export function parseMarkdown(src: string, options?: ParseMarkdownOptions): string {
  if (!src.trim()) return ''
  const streamingCharts = options?.streamingCharts === true
  const prepared = streamingCharts ? stabilizeStreamingChartFences(src) : src
  const fixed = prepared.replace(/(\|[^\n]*\|\s*\n)(?=[^\s|])/g, '$1\n')
  parseStreamingCharts = streamingCharts
  try {
    return marked.parse(fixed) as string
  } finally {
    parseStreamingCharts = false
  }
}
