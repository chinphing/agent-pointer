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
  cleaned = fitSvgViewBoxToAttributedContent(cleaned)
  return { ok: true, svg: cleaned }
}

export function tryParseSvgFence(
  raw: string
): { ok: true; svg: string } | { ok: false; reason: string } {
  return sanitizeSvgMarkup(raw)
}

/**
 * Prefer the authored `viewBox` size for layout so chat `width="100%"` does not
 * crush dense diagrams (long labels stay readable; frame scrolls horizontally).
 * Returns CSS pixel width/height when viewBox is usable.
 */
export function intrinsicSvgSizeFromViewBox(
  viewBox: string | null | undefined
): { x: number; y: number; width: number; height: number } | null {
  if (!viewBox) return null
  const parts = viewBox
    .trim()
    .split(/[\s,]+/)
    .map(p => Number(p))
  if (parts.length !== 4) return null
  const [x, y, w, h] = parts
  if (
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !Number.isFinite(w) ||
    !Number.isFinite(h) ||
    w <= 0 ||
    h <= 0
  ) {
    return null
  }
  return { x, y, width: w, height: h }
}

const VIEWBOX_FIT_PAD = 16

/**
 * Models often undersize `viewBox` (content drawn below the box → bottom clipped).
 * Expand the viewBox from common geometry attributes before mount.
 */
export function fitSvgViewBoxToAttributedContent(svg: string): string {
  const vbMatch = svg.match(/\bviewBox\s*=\s*(["'])([^"']+)\1/i)
  if (!vbMatch) return svg
  const current = intrinsicSvgSizeFromViewBox(vbMatch[2])
  if (!current) return svg

  let minX = current.x
  let minY = current.y
  let maxX = current.x + current.width
  let maxY = current.y + current.height

  const num = (raw: string | undefined, fallback = 0) => {
    if (raw == null || raw === '') return fallback
    const n = Number(raw)
    return Number.isFinite(n) ? n : fallback
  }

  // rect / image / use with x/y/width/height
  const boxRe =
    /<(?:rect|image|use|foreignObject)\b([^>]*?)(?:\/>|>)/gi
  for (const m of svg.matchAll(boxRe)) {
    const attrs = m[1] || ''
    const x = num(attrs.match(/\bx\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    const y = num(attrs.match(/\by\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    const w = num(attrs.match(/\bwidth\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    const h = num(attrs.match(/\bheight\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    if (w <= 0 && h <= 0) continue
    minX = Math.min(minX, x)
    minY = Math.min(minY, y)
    maxX = Math.max(maxX, x + Math.max(w, 0))
    maxY = Math.max(maxY, y + Math.max(h, 0))
  }

  // circle
  for (const m of svg.matchAll(/<circle\b([^>]*?)(?:\/>|>)/gi)) {
    const attrs = m[1] || ''
    const cx = num(attrs.match(/\bcx\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    const cy = num(attrs.match(/\bcy\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    const r = num(attrs.match(/\br\s*=\s*["']?(-?[\d.]+)/i)?.[1])
    minX = Math.min(minX, cx - r)
    minY = Math.min(minY, cy - r)
    maxX = Math.max(maxX, cx + r)
    maxY = Math.max(maxY, cy + r)
  }

  // text / tspan baseline ≈ y; reserve ~font-size (default 12) below baseline
  for (const m of svg.matchAll(/<(?:text|tspan)\b([^>]*?)(?:\/>|>)/gi)) {
    const attrs = m[1] || ''
    const x = num(attrs.match(/\bx\s*=\s*["']?(-?[\d.]+)/i)?.[1], Number.NaN)
    const y = num(attrs.match(/\by\s*=\s*["']?(-?[\d.]+)/i)?.[1], Number.NaN)
    if (!Number.isFinite(y)) continue
    const fs = num(attrs.match(/\bfont-size\s*=\s*["']?(-?[\d.]+)/i)?.[1], 12)
    if (Number.isFinite(x)) {
      minX = Math.min(minX, x)
      maxX = Math.max(maxX, x)
    }
    minY = Math.min(minY, y - fs)
    maxY = Math.max(maxY, y + fs * 0.35)
  }

  // path / polyline / polygon / line — sample numeric pairs and single H/V
  for (const m of svg.matchAll(/<(?:path|polyline|polygon|line)\b([^>]*?)(?:\/>|>)/gi)) {
    const attrs = m[1] || ''
    const d = attrs.match(/\bd\s*=\s*(["'])([\s\S]*?)\1/i)?.[2]
    const points = attrs.match(/\bpoints\s*=\s*(["'])([\s\S]*?)\1/i)?.[2]
    const blob = `${d || ''} ${points || ''}`
    const x1 = attrs.match(/\bx1\s*=\s*["']?(-?[\d.]+)/i)?.[1]
    const y1 = attrs.match(/\by1\s*=\s*["']?(-?[\d.]+)/i)?.[1]
    const x2 = attrs.match(/\bx2\s*=\s*["']?(-?[\d.]+)/i)?.[1]
    const y2 = attrs.match(/\by2\s*=\s*["']?(-?[\d.]+)/i)?.[1]
    if (x1 != null && y1 != null) {
      minX = Math.min(minX, num(x1))
      minY = Math.min(minY, num(y1))
      maxX = Math.max(maxX, num(x1))
      maxY = Math.max(maxY, num(y1))
    }
    if (x2 != null && y2 != null) {
      minX = Math.min(minX, num(x2))
      minY = Math.min(minY, num(y2))
      maxX = Math.max(maxX, num(x2))
      maxY = Math.max(maxY, num(y2))
    }
    const nums = blob.match(/-?[\d.]+/g)?.map(Number).filter(Number.isFinite) || []
    for (let i = 0; i + 1 < nums.length; i += 2) {
      const px = nums[i]!
      const py = nums[i + 1]!
      // Heuristic: skip tiny marker path coords inside defs (usually < 12)
      if (Math.abs(px) <= 12 && Math.abs(py) <= 12 && nums.length <= 8) continue
      minX = Math.min(minX, px)
      minY = Math.min(minY, py)
      maxX = Math.max(maxX, px)
      maxY = Math.max(maxY, py)
    }
  }

  const nextX = Math.min(current.x, minX - VIEWBOX_FIT_PAD)
  const nextY = Math.min(current.y, minY - VIEWBOX_FIT_PAD)
  const nextMaxX = Math.max(current.x + current.width, maxX + VIEWBOX_FIT_PAD)
  const nextMaxY = Math.max(current.y + current.height, maxY + VIEWBOX_FIT_PAD)
  const nextW = nextMaxX - nextX
  const nextH = nextMaxY - nextY

  if (
    nextW <= current.width + 0.5 &&
    nextH <= current.height + 0.5 &&
    nextX >= current.x - 0.5 &&
    nextY >= current.y - 0.5
  ) {
    return svg
  }

  const nextVb = `${roundVb(nextX)} ${roundVb(nextY)} ${roundVb(nextW)} ${roundVb(nextH)}`
  return svg.replace(vbMatch[0], `viewBox=${vbMatch[1]}${nextVb}${vbMatch[1]}`)
}

function roundVb(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(2).replace(/\.?0+$/, '')
}

/**
 * Apply mount-time layout defaults on an imported SVG root (browser only).
 * - Size from viewBox when present (ignore percentage width that collapses labels)
 * - Keep vector overflow visible so markers / edge labels are not clipped
 * - If still painted outside the box, expand viewBox via getBBox
 */
export function applySvgMountLayout(root: SVGElement): void {
  expandSvgViewBoxFromDomBBox(root)
  const size = intrinsicSvgSizeFromViewBox(root.getAttribute('viewBox'))
  if (size) {
    root.setAttribute('width', String(size.width))
    root.setAttribute('height', String(size.height))
    root.style.width = `${size.width}px`
    root.style.height = 'auto'
    root.style.maxWidth = 'none'
  }
  if (!root.getAttribute('overflow')) {
    root.setAttribute('overflow', 'visible')
  }
}

/** Expand viewBox using rendered geometry (covers path-heavy diagrams). */
export function expandSvgViewBoxFromDomBBox(root: SVGElement): void {
  const current = intrinsicSvgSizeFromViewBox(root.getAttribute('viewBox'))
  if (!current) return
  let minX = current.x
  let minY = current.y
  let maxX = current.x + current.width
  let maxY = current.y + current.height
  let saw = false
  try {
    for (const node of Array.from(root.querySelectorAll('*'))) {
      if (!(node instanceof SVGGraphicsElement)) continue
      if (node.closest('defs')) continue
      if (typeof node.getBBox !== 'function') continue
      let b: DOMRect
      try {
        b = node.getBBox()
      } catch {
        continue
      }
      if (!Number.isFinite(b.x) || !Number.isFinite(b.y)) continue
      if (b.width === 0 && b.height === 0) continue
      saw = true
      minX = Math.min(minX, b.x)
      minY = Math.min(minY, b.y)
      maxX = Math.max(maxX, b.x + b.width)
      maxY = Math.max(maxY, b.y + b.height)
    }
  } catch (err) {
    console.warn('[markdownSvg] getBBox viewBox fit skipped', err)
    return
  }
  if (!saw) return

  const nextX = Math.min(current.x, minX - VIEWBOX_FIT_PAD)
  const nextY = Math.min(current.y, minY - VIEWBOX_FIT_PAD)
  const nextMaxX = Math.max(current.x + current.width, maxX + VIEWBOX_FIT_PAD)
  const nextMaxY = Math.max(current.y + current.height, maxY + VIEWBOX_FIT_PAD)
  const nextW = nextMaxX - nextX
  const nextH = nextMaxY - nextY
  if (
    nextW <= current.width + 0.5 &&
    nextH <= current.height + 0.5 &&
    nextX >= current.x - 0.5 &&
    nextY >= current.y - 0.5
  ) {
    return
  }
  root.setAttribute(
    'viewBox',
    `${roundVb(nextX)} ${roundVb(nextY)} ${roundVb(nextW)} ${roundVb(nextH)}`
  )
}
