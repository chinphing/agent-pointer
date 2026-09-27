import { t } from '../i18n'
/** Mermaid fence helpers for markdown (`mermaid`). */

import { canvasRgb } from './markdownChart'
import { encodeSvgConfigAttr } from './markdownSvg'

export const MERMAID_FENCE_LANGS = new Set(['mermaid'])
export const MAX_MERMAID_BYTES = 100_000

export function isMermaidFenceLang(lang: string): boolean {
  return MERMAID_FENCE_LANGS.has(lang.toLowerCase())
}

/** Fixed Mermaid body for streaming placeholders (valid syntax, marked pending). */
export const STREAMING_MERMAID_STUB =
  `%% pointer-mermaid-pending\nflowchart TD\n  A[${t('markdown.mermaidPendingNode')}]`

export const STREAMING_MERMAID_FENCE =
  '```mermaid\n' + STREAMING_MERMAID_STUB + '\n```'

export const STREAMING_MERMAID_HOST_HTML =
  `<div class="md-mermaid group md-mermaid--pending" data-mermaid-config="${encodeSvgConfigAttr(STREAMING_MERMAID_STUB)}">` +
  `<div class="md-mermaid-toolbar" hidden></div>` +
  `<div class="md-mermaid-frame"><div class="md-mermaid-status md-mermaid-status-pending">${t('markdown.diagramGenerating')}</div></div>` +
  `<pre class="md-mermaid-source" hidden></pre>` +
  `</div>\n`

export function mermaidHostHtml(raw: string): string {
  const encoded = encodeSvgConfigAttr(raw.trim())
  return (
    `<div class="md-mermaid group" data-mermaid-config="${encoded}">` +
    `<div class="md-mermaid-toolbar"></div>` +
    `<div class="md-mermaid-frame"></div>` +
    `<pre class="md-mermaid-source" hidden></pre>` +
    `</div>\n`
  )
}

export function isStreamingMermaidStub(raw: string): boolean {
  return raw.includes('pointer-mermaid-pending')
}

export function mermaidThemeScheme(): 'light' | 'dark' {
  if (typeof document === 'undefined') return 'light'
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light'
}

export function mermaidThemeCacheKey(encoded: string): string {
  return `${mermaidThemeScheme()}|${encoded}`
}

/**
 * Drop diagram-level theme directives so the host palette always wins.
 * Nested `{…}` inside init is fine; terminator is `}%%`.
 */
export function stripMermaidHostThemeOverrides(raw: string): string {
  return raw.replace(/%%\{\s*init(?:ialize)?\s*:[\s\S]*?\}%%/gi, '').trim()
}

/** True when an unquoted `[]` label would confuse the flowchart parser. */
export function flowchartLabelNeedsQuotes(inner: string): boolean {
  const t = inner.trim()
  if (!t) return false
  return /[()[\]{}<>|*\\/=+#;,→←]|<br|\n/i.test(t)
}

/**
 * Unquoted subgraph titles may only be letters, numbers, spaces, `_`, `-`.
 * Fullwidth punctuation (`（，）`) is a lexical error; ASCII `(` starts a stadium.
 */
export function subgraphTitleNeedsQuotes(title: string): boolean {
  const t = title.trim()
  if (!t) return false
  return /[^\p{L}\p{N}\s_-]/u.test(t)
}

/** Close `]` for `id[…]`, counting nested `vouchers[]` / `matches[]`. */
function findFlowchartLabelClose(source: string, contentStart: number): number {
  let depth = 1
  for (let i = contentStart; i < source.length; i++) {
    const c = source[i]
    if (c === '\n') return -1
    if (c === '[') depth += 1
    else if (c === ']') {
      depth -= 1
      if (depth === 0) return i
    }
  }
  return -1
}

/**
 * Models often put `/`, `()`, `*`, `[]`, or `<br/>` in `A[label]` without quotes.
 * Unquoted `(` is parsed as a stadium node and the whole diagram fails.
 * A naive `[^\]]*` cut at the first `]` turns `vouchers[]` into a parse error.
 */
export function quoteFlowchartNodeLabels(source: string): string {
  if (!/^\s*(?:flowchart|graph)\b/im.test(source)) return source
  const open = /(^|[\s;])([A-Za-z][\w-]*)\[(?!\s*")/gm
  let out = ''
  let last = 0
  let m: RegExpExecArray | null
  while ((m = open.exec(source))) {
    const contentStart = m.index + m[0].length
    const close = findFlowchartLabelClose(source, contentStart)
    if (close < 0) continue
    const inner = source.slice(contentStart, close)
    const prefix = m[1]!
    const id = m[2]!
    const normalized = inner.replace(/<br\s*\/?>/gi, '<br>')
    out += source.slice(last, m.index)
    if (!flowchartLabelNeedsQuotes(normalized)) {
      out += source.slice(m.index, close + 1)
    } else {
      out += `${prefix}${id}["${normalized.replace(/"/g, '#quot;')}"]`
    }
    last = close + 1
    open.lastIndex = last
  }
  return out + source.slice(last)
}

/**
 * Models write `subgraph 明细路径（入账，正确）` without quotes.
 * Mermaid's lexer rejects fullwidth `（，）` and treats ASCII `(` as a stadium.
 */
export function quoteFlowchartSubgraphTitles(source: string): string {
  if (!/^\s*(?:flowchart|graph)\b/im.test(source)) return source
  return source.replace(
    /^([ \t]*)subgraph[ \t]+(.+?)[ \t]*$/gm,
    (full, indent: string, rest: string) => {
      const trimmed = rest.trim()
      if (trimmed.startsWith('"')) return full

      const withId = /^([A-Za-z][\w-]*)\s*\[([\s\S]*)\]$/.exec(trimmed)
      if (withId) {
        const id = withId[1]!
        const inner = withId[2]!
        if (/^\s*"/.test(inner)) return full
        if (!flowchartLabelNeedsQuotes(inner) && !subgraphTitleNeedsQuotes(inner)) {
          return full
        }
        return `${indent}subgraph ${id}["${inner.replace(/"/g, '#quot;')}"]`
      }

      if (!subgraphTitleNeedsQuotes(trimmed)) return full
      return `${indent}subgraph "${trimmed.replace(/"/g, '#quot;')}"`
    }
  )
}

/** Strip host-theme init and quote fragile flowchart labels (model output). */
export function prepareMermaidSource(raw: string): string {
  const stripped = stripMermaidHostThemeOverrides(raw)
  const quotedNodes = quoteFlowchartNodeLabels(stripped)
  const quoted = quoteFlowchartSubgraphTitles(quotedNodes)
  if (quoted !== stripped) {
    console.info('[markdownMermaid] quoted flowchart labels for parse')
  }
  return quoted
}

function mermaidToken(varName: string, light: string, dark: string): string {
  return canvasRgb(varName, mermaidThemeScheme() === 'dark' ? dark : light)
}

/** Mermaid `themeVariables` from app CSS tokens (same family as charts). */
export function mermaidThemeVariables(): Record<string, string | boolean | number> {
  const dark = mermaidThemeScheme() === 'dark'
  const foreground = mermaidToken('--foreground', '240 6% 10%', '240 6% 96%')
  const fence = mermaidToken('--fence-bg', '0 0% 100%', '240 4% 8%')
  const cluster = mermaidToken('--mermaid-cluster', '240 5% 96%', '240 5% 16%')
  const node = mermaidToken('--mermaid-node', '0 0% 100%', '240 5% 22%')
  const nodeBorder = mermaidToken('--mermaid-node-border', '240 8% 82%', '240 8% 42%')
  const edge = mermaidToken('--mermaid-edge', '240 6% 68%', '240 6% 72%')
  const accent = mermaidToken('--accent', '211 100% 46%', '211 100% 58%')
  const danger = mermaidToken('--danger', '4 78% 50%', '4 72% 58%')

  return {
    darkMode: dark,
    background: fence,
    primaryColor: node,
    primaryTextColor: foreground,
    primaryBorderColor: nodeBorder,
    secondaryColor: cluster,
    secondaryTextColor: foreground,
    secondaryBorderColor: nodeBorder,
    tertiaryColor: cluster,
    tertiaryTextColor: foreground,
    tertiaryBorderColor: nodeBorder,
    lineColor: edge,
    textColor: foreground,
    mainBkg: node,
    nodeBkg: node,
    nodeBorder,
    clusterBkg: cluster,
    clusterBorder: nodeBorder,
    titleColor: foreground,
    edgeLabelBackground: fence,
    nodeTextColor: foreground,
    defaultLinkColor: edge,
    actorBkg: node,
    actorBorder: nodeBorder,
    actorTextColor: foreground,
    actorLineColor: edge,
    signalColor: edge,
    signalTextColor: foreground,
    labelBoxBkgColor: fence,
    labelBoxBorderColor: nodeBorder,
    labelTextColor: foreground,
    loopTextColor: foreground,
    activationBkgColor: cluster,
    activationBorderColor: nodeBorder,
    sequenceNumberColor: foreground,
    noteBkgColor: cluster,
    noteTextColor: foreground,
    noteBorderColor: nodeBorder,
    sectionBkgColor: cluster,
    altSectionBkgColor: fence,
    sectionBkgColor2: node,
    taskBkgColor: node,
    taskTextColor: foreground,
    taskTextLightColor: foreground,
    taskTextDarkColor: foreground,
    taskBorderColor: nodeBorder,
    activeTaskBkgColor: accent,
    activeTaskBorderColor: accent,
    doneTaskBkgColor: cluster,
    doneTaskBorderColor: nodeBorder,
    critBkgColor: danger,
    critBorderColor: danger,
    gridColor: nodeBorder,
    todayLineColor: accent,
    errorBkgColor: danger,
    errorTextColor: foreground,
    attributeBackgroundColorOdd: fence,
    attributeBackgroundColorEven: cluster,
    relationColor: edge,
    relationLabelBackground: fence,
    classText: foreground,
    fontFamily: 'ui-sans-serif, system-ui, sans-serif',
    useGradient: false,
    dropShadow: 'none',
    strokeWidth: 1.5,
  }
}

/** Flowchart `[]` nodes are sharp rects; `{decision}` diamonds are sharp polygons. */
export const MERMAID_NODE_RX = 8
export const MERMAID_CLUSTER_RX = 10

type SvgPoint = { x: number; y: number }

function parseSvgPoints(points: string): SvgPoint[] {
  const nums = points
    .trim()
    .split(/[\s,]+/)
    .map(Number)
    .filter(n => Number.isFinite(n))
  const pts: SvgPoint[] = []
  for (let i = 0; i + 1 < nums.length; i += 2) {
    pts.push({ x: nums[i]!, y: nums[i + 1]! })
  }
  return pts
}

function roundedPolygonPathD(pts: SvgPoint[], radius: number): string | null {
  const n = pts.length
  if (n < 3) return null
  const segs: string[] = []
  for (let i = 0; i < n; i++) {
    const prev = pts[(i - 1 + n) % n]!
    const curr = pts[i]!
    const next = pts[(i + 1) % n]!
    const dIn = Math.hypot(curr.x - prev.x, curr.y - prev.y)
    const dOut = Math.hypot(next.x - curr.x, next.y - curr.y)
    if (dIn < 0.5 || dOut < 0.5) return null
    const r = Math.min(radius, dIn / 2, dOut / 2)
    const ax = curr.x - ((curr.x - prev.x) / dIn) * r
    const ay = curr.y - ((curr.y - prev.y) / dIn) * r
    const bx = curr.x + ((next.x - curr.x) / dOut) * r
    const by = curr.y + ((next.y - curr.y) / dOut) * r
    if (i === 0) segs.push(`M${ax.toFixed(2)} ${ay.toFixed(2)}`)
    else segs.push(`L${ax.toFixed(2)} ${ay.toFixed(2)}`)
    segs.push(`Q${curr.x.toFixed(2)} ${curr.y.toFixed(2)} ${bx.toFixed(2)} ${by.toFixed(2)}`)
  }
  segs.push('Z')
  return segs.join(' ')
}

function roundNodePolygons(root: Element): void {
  const ns = 'http://www.w3.org/2000/svg'
  for (const polygon of Array.from(root.querySelectorAll('.node polygon'))) {
    if (!(polygon instanceof Element)) continue
    if (polygon.closest('.edgeLabel') || polygon.closest('.cluster')) continue
    const pts = parseSvgPoints(polygon.getAttribute('points') ?? '')
    if (pts.length < 4) continue
    const d = roundedPolygonPathD(pts, MERMAID_NODE_RX)
    if (!d) continue
    const path = root.ownerDocument?.createElementNS(ns, 'path')
    if (!path) continue
    for (const attr of Array.from(polygon.attributes)) {
      if (attr.name === 'points') continue
      path.setAttribute(attr.name, attr.value)
    }
    path.setAttribute('d', d)
    polygon.replaceWith(path)
  }
}

/**
 * Round flowchart node/cluster rects and decision diamonds so export matches
 * on-screen rounding. Skips label/edge backplates.
 */
export function roundMermaidSvgRects(svg: string): string {
  if (typeof DOMParser === 'undefined' || typeof XMLSerializer === 'undefined') {
    return svg
  }
  try {
    const doc = new DOMParser().parseFromString(svg, 'image/svg+xml')
    if (doc.querySelector('parsererror')) return svg
    const root = doc.documentElement
    if (!root || root.localName.toLowerCase() !== 'svg') return svg
    for (const rect of Array.from(root.querySelectorAll('rect'))) {
      if (!(rect instanceof Element)) continue
      const cls = rect.getAttribute('class') ?? ''
      if (cls.split(/\s+/).includes('text')) continue
      if (rect.closest('.edgeLabel')) continue
      if (rect.closest('.label') && !rect.closest('.label-container')) continue
      const inCluster = Boolean(rect.closest('.cluster'))
      const inNode = Boolean(rect.closest('.node'))
      if (!inCluster && !inNode) continue
      const r = inCluster && !inNode ? String(MERMAID_CLUSTER_RX) : String(MERMAID_NODE_RX)
      rect.setAttribute('rx', r)
      rect.setAttribute('ry', r)
    }
    roundNodePolygons(root)
    return new XMLSerializer().serializeToString(root)
  } catch (err) {
    console.warn('[markdownMermaid] round shapes skipped', err)
    return svg
  }
}

export function mermaidInitializeConfig() {
  return {
    startOnLoad: false,
    securityLevel: 'strict' as const,
    // Pure SVG labels when possible. Also pin flowchart.htmlLabels: some
    // Mermaid 11 paths still read that flag and default it to true.
    htmlLabels: false,
    flowchart: { htmlLabels: false, useMaxWidth: true, padding: 12 },
    theme: 'base' as const,
    darkMode: mermaidThemeScheme() === 'dark',
    look: 'classic' as const,
    themeVariables: mermaidThemeVariables(),
  }
}
