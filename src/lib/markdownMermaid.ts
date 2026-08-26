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
  '%% pointer-mermaid-pending\nflowchart TD\n  A[图示生成中]'

export const STREAMING_MERMAID_FENCE =
  '```mermaid\n' + STREAMING_MERMAID_STUB + '\n```'

export const STREAMING_MERMAID_HOST_HTML =
  `<div class="md-mermaid group md-mermaid--pending" data-mermaid-config="${encodeSvgConfigAttr(STREAMING_MERMAID_STUB)}">` +
  `<div class="md-mermaid-toolbar" hidden></div>` +
  `<div class="md-mermaid-frame"><div class="md-mermaid-status md-mermaid-status-pending">图示生成中…</div></div>` +
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

function mermaidToken(varName: string, light: string, dark: string): string {
  return canvasRgb(varName, mermaidThemeScheme() === 'dark' ? dark : light)
}

/** Mermaid `themeVariables` from app CSS tokens (same family as charts). */
export function mermaidThemeVariables(): Record<string, string | boolean | number> {
  const dark = mermaidThemeScheme() === 'dark'
  const foreground = mermaidToken('--foreground', '240 6% 10%', '240 6% 96%')
  const card = mermaidToken('--card', '0 0% 100%', '240 4% 11%')
  const cluster = mermaidToken('--mermaid-cluster', '240 5% 94%', '240 6% 20%')
  const node = mermaidToken('--mermaid-node', '211 40% 93%', '211 32% 30%')
  const nodeBorder = mermaidToken('--mermaid-node-border', '240 8% 78%', '240 8% 48%')
  const edge = mermaidToken('--mermaid-edge', '240 5% 58%', '240 8% 80%')
  const accent = mermaidToken('--accent', '211 100% 46%', '211 100% 58%')
  const danger = mermaidToken('--danger', '4 78% 50%', '4 72% 58%')

  return {
    darkMode: dark,
    background: card,
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
    edgeLabelBackground: card,
    nodeTextColor: foreground,
    defaultLinkColor: edge,
    actorBkg: node,
    actorBorder: nodeBorder,
    actorTextColor: foreground,
    actorLineColor: edge,
    signalColor: edge,
    signalTextColor: foreground,
    labelBoxBkgColor: card,
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
    altSectionBkgColor: card,
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
    attributeBackgroundColorOdd: card,
    attributeBackgroundColorEven: cluster,
    relationColor: edge,
    relationLabelBackground: card,
    classText: foreground,
    fontFamily: 'ui-sans-serif, system-ui, sans-serif',
    useGradient: false,
    dropShadow: 'none',
    strokeWidth: 2,
  }
}

export function mermaidInitializeConfig() {
  return {
    startOnLoad: false,
    securityLevel: 'strict' as const,
    // Pure SVG labels when possible. Also pin flowchart.htmlLabels: some
    // Mermaid 11 paths still read that flag and default it to true.
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    theme: 'base' as const,
    darkMode: mermaidThemeScheme() === 'dark',
    look: 'classic' as const,
    themeVariables: mermaidThemeVariables(),
  }
}
