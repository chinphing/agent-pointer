/**
 * Fenced `html` / `htm` blocks in chat Markdown — render as HTML (not a code card).
 * Prefer for tables that need column widths (`colgroup` / `%`); keep dangerous tags out.
 *
 * Sanitization: regex first pass (works in Node), DOMParser second pass when
 * available (browser/Tauri). The DOM pass catches what regex misses: nested
 * obfuscation like `<scr<script>ipt>`, attributes with `>` in quoted strings, etc.
 */

const DANGEROUS_TAGS =
  /<\/?(?:script|iframe|object|embed|link|meta|base|form|svg|math|style|template)\b[^>]*>/gi

const EVENT_HANDLER_ATTR = /\s+on[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi

const JS_URL_ATTR =
  /\b(href|src|xlink:href|action)\s*=\s*(["'])\s*(?:javascript|vbscript|data):[^"']*\2/gi

const BLOCKED_TAGS = new Set([
  'script', 'iframe', 'object', 'embed', 'link', 'meta', 'base',
  'form', 'svg', 'math', 'style', 'template',
])

const URL_ATTRS = new Set(['href', 'src', 'xlink:href', 'action'])

export function isHtmlFenceLang(lang: string): boolean {
  const normalized = lang.trim().toLowerCase()
  return normalized === 'html' || normalized === 'htm'
}

/** Strip high-risk tags/attrs before mounting html fences into `v-html`. */
export function sanitizeHtmlFence(raw: string): string {
  let html = raw.trim()
  if (!html) return ''

  // Regex first pass — fast, works everywhere.
  html = html.replace(DANGEROUS_TAGS, '')
  html = html.replace(EVENT_HANDLER_ATTR, '')
  html = html.replace(JS_URL_ATTR, '$1="#"')

  // DOMParser second pass — catches nested obfuscation and malformed HTML.
  if (typeof DOMParser !== 'undefined') {
    try {
      const doc = new DOMParser().parseFromString(html, 'text/html')
      const walk = doc.body.querySelectorAll('*')
      for (const el of Array.from(walk)) {
        if (BLOCKED_TAGS.has(el.localName.toLowerCase())) {
          el.remove()
          continue
        }
        for (const attr of Array.from(el.attributes)) {
          const name = attr.name.toLowerCase()
          if (name.startsWith('on')) {
            el.removeAttribute(attr.name)
            continue
          }
          if (URL_ATTRS.has(name)) {
            const value = attr.value.trim().toLowerCase()
            if (
              value.startsWith('javascript:') ||
              value.startsWith('vbscript:') ||
              value.startsWith('data:')
            ) {
              el.setAttribute(attr.name, '#')
            }
          }
        }
      }
      html = doc.body.innerHTML
    } catch {
      // DOMParser failed — regex result is the fallback.
    }
  }

  return html
}
