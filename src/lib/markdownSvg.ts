/** SVG fence helpers for markdown (`svg`). */

import { decodeChartConfigAttr, encodeChartConfigAttr } from './markdownChart'

export const SVG_FENCE_LANGS = new Set(['svg'])
export const MAX_SVG_BYTES = 200_000

const FORBIDDEN_TAGS = new Set([
  'script',
  'foreignobject',
  'iframe',
  'embed',
  'object',
  'link',
  'meta',
  'base',
])

const URL_ATTRS = new Set([
  'href',
  'xlink:href',
  'src',
  'poster',
  'action',
  'formaction',
  'xlink:src',
])

export function isSvgFenceLang(lang: string): boolean {
  return SVG_FENCE_LANGS.has(lang.toLowerCase())
}

export function encodeSvgConfigAttr(svg: string): string {
  return encodeChartConfigAttr(svg)
}

export function decodeSvgConfigAttr(encoded: string): string | null {
  return decodeChartConfigAttr(encoded)
}

function stripForbiddenElementsByRegex(src: string): string {
  let out = src
  for (const tag of FORBIDDEN_TAGS) {
    const re = new RegExp(`<${tag}\\b[^>]*(?:/>|>[\\s\\S]*?</${tag}\\s*>)`, 'gi')
    out = out.replace(re, '')
  }
  return out
}

function stripEventHandlersByRegex(src: string): string {
  return src.replace(/\s+on[a-zA-Z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/g, '')
}

function isSafeUrlValue(value: string): boolean {
  const v = value.trim()
  if (!v || v.startsWith('#')) return true
  const lower = v.toLowerCase()
  if (
    lower.startsWith('javascript:') ||
    lower.startsWith('vbscript:') ||
    lower.startsWith('data:')
  ) {
    return false
  }
  // Block remote / absolute network fetches from diagram markup.
  if (/^[a-z][a-z0-9+.-]*:/i.test(v)) return false
  return true
}

function neutralizeDangerousUrlsByRegex(src: string): string {
  return src.replace(
    /\s((?:xlink:)?(?:href|src)|poster|action|formaction)\s*=\s*(["'])([\s\S]*?)\2/gi,
    (full, attr: string, quote: string, value: string) => {
      if (isSafeUrlValue(value)) return full
      return ` ${attr}=${quote}${quote}`
    }
  )
}

function stripStyleExpressionsByRegex(src: string): string {
  return src.replace(/\sstyle\s*=\s*(["'])([\s\S]*?)\1/gi, (full, quote: string, value: string) => {
    const lower = value.toLowerCase()
    if (
      lower.includes('expression(') ||
      lower.includes('javascript:') ||
      lower.includes('vbscript:') ||
      /url\s*\(\s*['"]?\s*data:/i.test(value)
    ) {
      return ''
    }
    return full
  })
}

/**
 * Validate + sanitize a fenced SVG body before any DOM insert.
 * Works in Node (regex path) and in the browser (DOMParser second pass when available).
 */
export function sanitizeSvgMarkup(
  raw: string
): { ok: true; svg: string } | { ok: false; reason: string } {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: false, reason: 'empty' }
  if (new TextEncoder().encode(trimmed).length > MAX_SVG_BYTES) {
    return { ok: false, reason: 'too_large' }
  }

  // Allow an optional XML declaration / doctype before a single <svg>…</svg> root.
  const svgStart = trimmed.search(/<\s*svg\b/i)
  if (svgStart < 0) return { ok: false, reason: 'not_svg' }
  const fromSvg = trimmed.slice(svgStart)
  if (!/^<\s*svg\b[\s\S]*<\/\s*svg\s*>\s*$/i.test(fromSvg)) {
    return { ok: false, reason: 'not_svg' }
  }

  const svgMatch = fromSvg.match(/<\s*svg\b[\s\S]*<\/\s*svg\s*>/i)
  if (!svgMatch) return { ok: false, reason: 'not_svg' }
  let cleaned = svgMatch[0]!
  cleaned = stripForbiddenElementsByRegex(cleaned)
  cleaned = stripEventHandlersByRegex(cleaned)
  cleaned = neutralizeDangerousUrlsByRegex(cleaned)
  cleaned = stripStyleExpressionsByRegex(cleaned)

  if (typeof DOMParser !== 'undefined') {
    try {
      const doc = new DOMParser().parseFromString(cleaned, 'image/svg+xml')
      const parseError = doc.querySelector('parsererror')
      if (parseError) return { ok: false, reason: 'parse_error' }
      const root = doc.documentElement
      if (!root || root.localName.toLowerCase() !== 'svg') {
        return { ok: false, reason: 'not_svg' }
      }
      const walk = root.querySelectorAll('*')
      for (const el of Array.from(walk)) {
        if (FORBIDDEN_TAGS.has(el.localName.toLowerCase())) {
          el.remove()
          continue
        }
        for (const attr of Array.from(el.attributes)) {
          const name = attr.name.toLowerCase()
          if (name.startsWith('on')) {
            el.removeAttribute(attr.name)
            continue
          }
          if (URL_ATTRS.has(name) && !isSafeUrlValue(attr.value)) {
            el.removeAttribute(attr.name)
          }
          if (name === 'style') {
            const lower = attr.value.toLowerCase()
            if (
              lower.includes('expression(') ||
              lower.includes('javascript:') ||
              /url\s*\(\s*['"]?\s*data:/i.test(attr.value)
            ) {
              el.removeAttribute(attr.name)
            }
          }
        }
      }
      cleaned = new XMLSerializer().serializeToString(root)
    } catch (err) {
      console.warn('[markdownSvg] DOMParser sanitize failed; using regex result', err)
    }
  }

  if (!/^<\s*svg[\s>]/i.test(cleaned) || !/<\/\s*svg\s*>\s*$/i.test(cleaned)) {
    return { ok: false, reason: 'not_svg' }
  }
  return { ok: true, svg: cleaned }
}

export function tryParseSvgFence(
  raw: string
): { ok: true; svg: string } | { ok: false; reason: string } {
  return sanitizeSvgMarkup(raw)
}
