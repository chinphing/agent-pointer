/**
 * Fenced `html` / `htm` blocks in chat Markdown — render as HTML (not a code card).
 * Prefer for tables that need column widths (`colgroup` / `%`); keep dangerous tags out.
 */

const DANGEROUS_TAGS =
  /<\/?(?:script|iframe|object|embed|link|meta|base|form|svg|math|style|template)\b[^>]*>/gi

const EVENT_HANDLER_ATTR = /\s+on[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi

const JS_URL_ATTR =
  /\b(href|src|xlink:href|action)\s*=\s*(["'])\s*(?:javascript|vbscript|data):[^"']*\2/gi

export function isHtmlFenceLang(lang: string): boolean {
  const normalized = lang.trim().toLowerCase()
  return normalized === 'html' || normalized === 'htm'
}

/** Strip high-risk tags/attrs before mounting html fences into `v-html`. */
export function sanitizeHtmlFence(raw: string): string {
  let html = raw.trim()
  if (!html) return ''
  html = html.replace(DANGEROUS_TAGS, '')
  html = html.replace(EVENT_HANDLER_ATTR, '')
  html = html.replace(JS_URL_ATTR, '$1="#"')
  return html
}
