/** Mermaid fence helpers for markdown (`mermaid`). */

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
