import { marked } from 'marked'
import {
  encodeChartConfigAttr,
  isChartFenceLang,
  tryParseChartConfig,
} from './markdownChart'
import {
  isHtmlFenceLang,
  sanitizeHtmlFence,
} from './markdownHtml'
import {
  encodeSvgConfigAttr,
  isSvgFenceLang,
  tryParseSvgFence,
} from './markdownSvg'

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

/** Fixed SVG body for streaming placeholders. */
export const STREAMING_SVG_STUB =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1" data-pointer-svg-pending="1"></svg>'

export const STREAMING_SVG_FENCE = '```svg\n' + STREAMING_SVG_STUB + '\n```'

export const STREAMING_SVG_HOST_HTML =
  `<div class="md-svg group md-svg--pending" data-svg-config="${encodeSvgConfigAttr(STREAMING_SVG_STUB)}">` +
  `<div class="md-svg-toolbar" hidden></div>` +
  `<div class="md-svg-frame"><div class="md-svg-status md-svg-status-pending">图示生成中…</div></div>` +
  `<pre class="md-svg-source" hidden></pre>` +
  `</div>\n`

/** Set only for the duration of `parseMarkdown(..., { streamingCharts/Svgs: true })`. */
let parseStreamingCharts = false
let parseStreamingSvgs = false

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/**
 * GFM tables already emit `<div class="table-wrapper"><table>…`.
 * Raw HTML `<table>` from the model does not — wrap those so theme chrome applies.
 */
export function wrapBareHtmlTables(html: string): string {
  if (!html.includes('<table')) return html
  return html.replace(
    /(?<!<div class="table-wrapper">)<table\b[\s\S]*?<\/table>/gi,
    table => `<div class="table-wrapper">${table}</div>`
  )
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

function svgHostHtml(raw: string, valid: boolean): string {
  const encoded = encodeSvgConfigAttr(raw.trim() || STREAMING_SVG_STUB)
  const stateClass = valid ? '' : ' md-svg--invalid'
  return (
    `<div class="md-svg group${stateClass}" data-svg-config="${encoded}">` +
    `<div class="md-svg-toolbar"></div>` +
    `<div class="md-svg-frame"></div>` +
    `<pre class="md-svg-source" hidden></pre>` +
    `</div>\n`
  )
}

const CHART_OPEN_RE = /^[ \t]*```(chartjs|chart)[ \t]*$/i
const SVG_OPEN_RE = /^[ \t]*```svg[ \t]*$/i
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

/**
 * Replace open/closed `svg` fences with a fixed stub so streaming re-parses
 * do not rewrite the SVG card HTML every token.
 */
export function stabilizeStreamingSvgFences(src: string): string {
  if (!src.includes('```')) return src
  const lines = src.split('\n')
  const out: string[] = []
  let i = 0
  let replaced = false
  while (i < lines.length) {
    const line = lines[i]!
    if (SVG_OPEN_RE.test(line)) {
      i += 1
      while (i < lines.length && !FENCE_CLOSE_RE.test(lines[i]!)) i += 1
      if (i < lines.length) i += 1
      out.push('```svg', STREAMING_SVG_STUB, '```')
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
      if (isSvgFenceLang(langString)) {
        if (parseStreamingSvgs) return STREAMING_SVG_HOST_HTML
        const raw = text.replace(/\n$/, '')
        if (raw.trim() === STREAMING_SVG_STUB) return STREAMING_SVG_HOST_HTML
        const parsed = tryParseSvgFence(raw)
        if (parsed.ok) return svgHostHtml(parsed.svg, true)
        return svgHostHtml(raw, false)
      }
      if (isHtmlFenceLang(langString)) {
        // Render ```html as HTML (tables with colgroup widths, etc.) — not a code card.
        const fragment = sanitizeHtmlFence(text.replace(/\n$/, ''))
        if (!fragment) return ''
        return `${wrapBareHtmlTables(fragment)}\n`
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
  /**
   * While the assistant turn is streaming, collapse svg fences to a fixed
   * pending host so `v-html` does not flash on every token.
   */
  streamingSvgs?: boolean
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
  const streamingSvgs = options?.streamingSvgs === true
  let prepared = src
  if (streamingCharts) prepared = stabilizeStreamingChartFences(prepared)
  if (streamingSvgs) prepared = stabilizeStreamingSvgFences(prepared)
  const fixed = prepared.replace(/(\|[^\n]*\|\s*\n)(?=[^\s|])/g, '$1\n')
  parseStreamingCharts = streamingCharts
  parseStreamingSvgs = streamingSvgs
  try {
    return wrapBareHtmlTables(marked.parse(fixed) as string)
  } finally {
    parseStreamingCharts = false
    parseStreamingSvgs = false
  }
}
