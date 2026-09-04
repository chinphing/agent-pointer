import { marked, type Token } from 'marked'
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
  isMermaidFenceLang,
  mermaidHostHtml,
  STREAMING_MERMAID_HOST_HTML,
  STREAMING_MERMAID_STUB,
} from './markdownMermaid'
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

const LIST_ITEM_RE = /^ {0,3}(?:[*+-]|\d{1,9}[.)])(?:[ \t]|$)/
const FENCE_MARKER_RE = /^ {0,3}(`{3,}|~{3,})/

/**
 * CommonMark lazy-continues an unindented line into the last list item.
 * Only close the list when the next line starts with `**` (bold section title);
 * other wraps stay in the item. Insert one blank line; skip fenced code.
 */
export function ensureBlankLineAfterListBeforeSection(src: string): string {
  if (!src.includes('\n')) return src
  const lines = src.split('\n')
  const out: string[] = []
  let inList = false
  let fence: string | null = null
  let changed = false

  for (const line of lines) {
    const fenceOpen = line.match(FENCE_MARKER_RE)
    if (fence) {
      out.push(line)
      if (
        fenceOpen &&
        fenceOpen[1]![0] === fence[0] &&
        fenceOpen[1]!.length >= fence.length
      ) {
        fence = null
      }
      continue
    }

    if (/^\s*$/.test(line)) {
      inList = false
      out.push(line)
      continue
    }

    if (LIST_ITEM_RE.test(line)) {
      inList = true
      out.push(line)
      continue
    }

    if (inList && line.startsWith('**')) {
      out.push('')
      changed = true
      inList = false
    }

    if (fenceOpen) fence = fenceOpen[1]!
    out.push(line)
  }

  return changed ? out.join('\n') : src
}

/**
 * CommonMark treats HTML blocks as opaque until a blank line. Models often emit:
 *   </table>
 *   ## Next heading
 * without a blank line, so subsequent Markdown is left unparsed. Insert gaps.
 */
export function ensureBlankLinesAroundHtmlTables(src: string): string {
  if (!src.includes('<table') && !src.includes('</table')) return src
  let out = src
  // Blank line before <table> when the previous line has content.
  out = out.replace(/([^\n])\n([ \t]*)(<table\b)/gi, '$1\n\n$2$3')
  // Blank line after </table> when the next line has content (not already blank).
  out = out.replace(/<\/table>([ \t]*)\n(?=[ \t]*\S)/gi, '</table>$1\n\n')
  return out
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
const MERMAID_OPEN_RE = /^[ \t]*```mermaid[ \t]*$/i
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
 * While streaming: stub **incomplete** `svg` fences only.
 * Closed fences are kept so the diagram can mount before the rest of the reply finishes.
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
      const openLine = line
      i += 1
      const bodyStart = i
      while (i < lines.length && !FENCE_CLOSE_RE.test(lines[i]!)) i += 1
      if (i < lines.length) {
        // Closed fence — keep authored body (stable once closed).
        out.push(openLine)
        for (let j = bodyStart; j <= i; j++) out.push(lines[j]!)
        i += 1
        continue
      }
      // Still open — fixed stub so growing tokens do not reshuffle card HTML.
      out.push('```svg', STREAMING_SVG_STUB, '```')
      replaced = true
      continue
    }
    out.push(line)
    i += 1
  }
  return replaced ? out.join('\n') : src
}

/**
 * While streaming: stub **incomplete** `mermaid` fences only.
 * Closed fences are kept so the diagram can mount before the rest of the reply finishes.
 */
export function stabilizeStreamingMermaidFences(src: string): string {
  if (!src.includes('```')) return src
  const lines = src.split('\n')
  const out: string[] = []
  let i = 0
  let replaced = false
  while (i < lines.length) {
    const line = lines[i]!
    if (MERMAID_OPEN_RE.test(line)) {
      const openLine = line
      i += 1
      const bodyStart = i
      while (i < lines.length && !FENCE_CLOSE_RE.test(lines[i]!)) i += 1
      if (i < lines.length) {
        // Closed fence — keep authored body (stable once closed).
        out.push(openLine)
        for (let j = bodyStart; j <= i; j++) out.push(lines[j]!)
        i += 1
        continue
      }
      // Still open — fixed stub so growing tokens do not reshuffle card HTML.
      out.push('```mermaid', STREAMING_MERMAID_STUB, '```')
      replaced = true
      continue
    }
    out.push(line)
    i += 1
  }
  return replaced ? out.join('\n') : src
}

/** After a leading `**…**`, these openers mean “title then body”. */
const PARAGRAPH_LEAD_SEP_RE = /^\s*(?:[—–―−]|-{1,2}\s|：|:)/

function significantParagraphTokens(tokens: Token[]): Token[] {
  return tokens.filter(t => {
    if (t.type === 'space') return false
    if (t.type === 'text' && !t.text.trim()) return false
    return true
  })
}

/**
 * LLM markdown often uses a lone `**Section**` or `**Item** — body`
 * instead of ATX headings. Classify so CSS can open the hierarchy.
 */
export function classifyMarkdownParagraph(
  tokens: Token[]
): 'title' | 'lead' | null {
  const sig = significantParagraphTokens(tokens)
  if (sig.length === 0) return null
  if (sig[0]!.type !== 'strong') return null
  if (sig.length === 1) return 'title'
  const first = sig[0]!
  const rest = tokens
    .slice(tokens.indexOf(first) + 1)
    .map(t => t.raw ?? '')
    .join('')
  return PARAGRAPH_LEAD_SEP_RE.test(rest) ? 'lead' : null
}

marked.use({
  renderer: {
    paragraph({ tokens }) {
      const html = this.parser.parseInline(tokens)
      const kind = classifyMarkdownParagraph(tokens)
      if (kind === 'title') {
        return `<p class="md-section-title">${html}</p>\n`
      }
      if (kind === 'lead') {
        const wrapped = html.replace(
          /^(<strong>[\s\S]*?<\/strong>)/,
          '<span class="md-lead">$1</span>'
        )
        return `<p>${wrapped}</p>\n`
      }
      return `<p>${html}</p>\n`
    },
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
      if (isMermaidFenceLang(langString)) {
        const raw = text.replace(/\n$/, '')
        if (raw.trim() === STREAMING_MERMAID_STUB) return STREAMING_MERMAID_HOST_HTML
        return mermaidHostHtml(raw)
      }
      if (isSvgFenceLang(langString)) {
        const raw = text.replace(/\n$/, '')
        if (raw.trim() === STREAMING_SVG_STUB) return STREAMING_SVG_HOST_HTML
        const parsed = tryParseSvgFence(raw)
        if (parsed.ok) return svgHostHtml(parsed.svg, true)
        // Unclosed fences are already stubbed. A closed body that fails
        // sanitize is invalid — do not keep the streaming placeholder.
        const incomplete = !raw.includes('</svg>')
        if (parseStreamingSvgs && incomplete) return STREAMING_SVG_HOST_HTML
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
      const langLabel = langString
        ? `<div class="fence-block-lang">${escapeHtml(langString)}</div>`
        : ''
      return (
        `<div class="code-block">${langLabel}<pre><code${langClass}>${body}</code></pre></div>\n`
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
  /**
   * While the assistant turn is streaming, stub **incomplete** mermaid fences
   * (closed ones are kept; the mount layer defers rendering until streaming ends).
   */
  streamingMermaid?: boolean
}

/**
 * Parse Markdown to HTML with shared configuration.
 *
 * Includes pre-processing that inserts blank lines after GFM / HTML tables, and
 * one blank line after a list when the next line starts with `**`, so CommonMark
 * does not swallow a bold section title into the last item.
 */
export function parseMarkdown(src: string, options?: ParseMarkdownOptions): string {
  if (!src.trim()) return ''
  const streamingCharts = options?.streamingCharts === true
  const streamingSvgs = options?.streamingSvgs === true
  const streamingMermaid = options?.streamingMermaid === true
  let prepared = src
  if (streamingCharts) prepared = stabilizeStreamingChartFences(prepared)
  if (streamingSvgs) prepared = stabilizeStreamingSvgFences(prepared)
  if (streamingMermaid) prepared = stabilizeStreamingMermaidFences(prepared)
  prepared = ensureBlankLineAfterListBeforeSection(prepared)
  prepared = ensureBlankLinesAroundHtmlTables(prepared)
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
