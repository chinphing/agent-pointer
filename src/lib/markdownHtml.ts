/**
 * Fenced `html` / `htm` blocks in chat Markdown — render as HTML (not a code card).
 * Prefer for tables that need column widths (`colgroup` / `%`); keep dangerous tags out.
 *
 * Sanitization: DOMPurify in the browser (battle-tested, catches nested
 * obfuscation and malformed HTML). Regex fallback for Node.js (tests).
 */

import DOMPurify from 'dompurify'

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
  const html = raw.trim()
  if (!html) return ''

  // Browser: DOMPurify handles everything (tags, attrs, URLs, nested tricks).
  if (typeof window !== 'undefined' && typeof DOMPurify.sanitize === 'function') {
    try {
      return DOMPurify.sanitize(html, {
        // Default config already strips script/on*/javascript:/iframe/etc.
        // FORBID_TAGS adds extra hardening for tags that are allowed by
        // default but we don't want in chat (svg/math can carry scripts).
        FORBID_TAGS: [
          'script', 'iframe', 'object', 'embed', 'link', 'meta',
          'base', 'form', 'svg', 'math', 'style', 'template',
        ],
      })
    } catch {
      // DOMPurify failed — fall through to regex.
    }
  }

  // Node.js fallback: regex strip (tests only; browser always has DOMPurify).
  let cleaned = html
  cleaned = cleaned.replace(DANGEROUS_TAGS, '')
  cleaned = cleaned.replace(EVENT_HANDLER_ATTR, '')
  cleaned = cleaned.replace(JS_URL_ATTR, '$1="#"')
  return cleaned
}
