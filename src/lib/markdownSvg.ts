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

  // Product-level flow-diagram fix: materialize `marker-end` arrowheads as
  // explicit polygons and snap arrow tips to the nearest rect edge, so rendered
  // connectors always touch their boxes (renderer-independent; fixes floating /
  // reversed arrowheads that models often draw with hand-computed coords).
  cleaned = normalizeFlowArrows(cleaned)

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

/** Max distance (px) an arrow tip may float from a rect edge and still be snapped onto it. */
const ARROW_SNAP_PX = 24
const ARROW_COLOR_FALLBACK = '#5F5E5A'

interface FlowRect {
  x: number
  y: number
  w: number
  h: number
}

interface ArrowMarkerSpec {
  vb: [number, number, number, number]
  refX: number
  refY: number
  mw: number
  mh: number
  strokeWidthUnits: boolean
  points: number[][]
  fill: string | null
  stroke: string | null
}

function attrNum(attrs: string, name: string, fallback: number): number {
  const m = attrs.match(
    new RegExp(`\\b${name}\\s*=\\s*["']?(-?\\d+(?:\\.\\d+)?)`, 'i')
  )
  if (!m) return fallback
  const n = Number(m[1])
  return Number.isFinite(n) ? n : fallback
}

function attrStr(attrs: string, name: string): string | null {
  const m = attrs.match(new RegExp(`\\b${name}\\s*=\\s*["']([^"']*)["']`, 'i'))
  return m ? m[1] : null
}

/** Parse absolute M/L coordinates from a path `d` (flow diagrams use M/L only). */
function pathPoints(d: string): number[][] {
  const nums =
    d.match(/-?\d*\.?\d+(?:e[-+]?\d+)?/gi)?.map(Number).filter(Number.isFinite) ?? []
  const pts: number[][] = []
  for (let i = 0; i + 1 < nums.length; i += 2) {
    pts.push([nums[i]!, nums[i + 1]!])
  }
  return pts
}

function round2(n: number): string {
  return String(Math.round(n * 100) / 100)
}

function rebuildPathD(
  pts: number[][],
  pxn: number,
  pyn: number,
  tx: number,
  ty: number
): string {
  const head = pts.slice(0, -2)
  const parts: string[] = []
  head.forEach(([x, y], i) => {
    parts.push(`${i === 0 ? 'M' : 'L'}${round2(x)} ${round2(y)}`)
  })
  if (head.length === 0) {
    parts.push(`M${round2(pxn)} ${round2(pyn)}`)
  } else {
    parts.push(`L${round2(pxn)} ${round2(pyn)}`)
  }
  parts.push(`L${round2(tx)} ${round2(ty)}`)
  return parts.join(' ')
}

function parseMarkerSpecs(svg: string): Map<string, ArrowMarkerSpec> {
  const specs = new Map<string, ArrowMarkerSpec>()
  const markerRe = /<marker\b([^>]*)>([\s\S]*?)<\/marker\s*>/gi
  for (const m of svg.matchAll(markerRe)) {
    const attrs = m[1] ?? ''
    const body = m[2] ?? ''
    const id = attrStr(attrs, 'id')
    if (!id) continue
    const vbRaw = attrStr(attrs, 'viewBox')
    const vb = vbRaw
      ? (vbRaw
          .trim()
          .split(/[\s,]+/)
          .map(Number) as [number, number, number, number])
      : null
    if (!vb || vb.length !== 4 || vb.some(n => !Number.isFinite(n)) || vb[2] <= 0 || vb[3] <= 0) {
      continue
    }
    const pathM = body.match(/<path\b([^>]*)>/i)
    if (!pathM) continue
    const pathAttrs = pathM[1] ?? ''
    const d = attrStr(pathAttrs, 'd')
    if (!d) continue
    const points = pathPoints(d)
    if (points.length < 2) continue
    specs.set(id, {
      vb,
      refX: attrNum(attrs, 'refX', vb[2] / 2),
      refY: attrNum(attrs, 'refY', vb[3] / 2),
      mw: attrNum(attrs, 'markerWidth', 3),
      mh: attrNum(attrs, 'markerHeight', 3),
      strokeWidthUnits: !/\bmarkerUnits\s*=\s*["']userSpaceOnUse["']/i.test(attrs),
      points,
      fill: attrStr(pathAttrs, 'fill'),
      stroke: attrStr(pathAttrs, 'stroke'),
    })
  }
  return specs
}

const PATH_TAG_RE = /<path\b([^>]*?)(\/>|<\/path\s*>)/gi

/** Replace `marker-end="url(#id)"` with an explicit `<polygon>` arrowhead. */
export function expandSvgArrowMarkers(svg: string): string {
  const specs = parseMarkerSpecs(svg)
  if (specs.size === 0) return svg
  return svg.replace(PATH_TAG_RE, (full, attrs: string, closing: string) => {
    const endRef = attrs.match(
      /\bmarker-end\s*=\s*["']url\(\s*#([^)]+)\)["']/i
    )
    if (!endRef) return full
    const spec = specs.get(endRef[1])
    if (!spec) return full
    const d = attrStr(attrs, 'd')
    if (!d) return full
    const pts = pathPoints(d)
    if (pts.length < 2) return full
    const [ex, ey] = pts[pts.length - 1]!
    const [px, py] = pts[pts.length - 2]!
    const dx = ex - px
    const dy = ey - py
    const len = Math.hypot(dx, dy)
    if (len < 1e-6) return full
    const angle = Math.atan2(dy, dx)
    const cos = Math.cos(angle)
    const sin = Math.sin(angle)
    const strokeWidth = attrNum(attrs, 'stroke-width', 1)
    const scale = (spec.strokeWidthUnits ? strokeWidth : 1) * (spec.mw / spec.vb[2])
    const color =
      spec.fill && spec.fill !== 'none' && !spec.fill.startsWith('url(')
        ? spec.fill
        : spec.stroke && spec.stroke !== 'none'
          ? spec.stroke
          : ARROW_COLOR_FALLBACK
    const polyPts = spec.points.map(([x, y]) => {
      const rx0 = (x - spec.refX) * scale
      const ry0 = (y - spec.refY) * scale
      return `${round2(rx0 * cos - ry0 * sin + ex)},${round2(rx0 * sin + ry0 * cos + ey)}`
    })
    const attrsClean = attrs.replace(/\s+marker-end\s*=\s*["'][^"']*["']/gi, '')
    return `<path${attrsClean}${closing}<polygon points="${polyPts.join(' ')}" fill="${color}"/>`
  })
}

interface RectEdgeSnap {
  edge: 'top' | 'bottom' | 'left' | 'right'
  tx: number
  ty: number
  dist: number
}

function nearestRectEdge(
  rects: FlowRect[],
  x: number,
  y: number
): RectEdgeSnap | null {
  let best: RectEdgeSnap | null = null
  const consider = (edge: RectEdgeSnap['edge'], tx: number, ty: number, dist: number) => {
    if (!best || dist < best.dist) best = { edge, tx, ty, dist }
  }
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v))
  for (const r of rects) {
    if (y >= r.y - ARROW_SNAP_PX && y <= r.y + r.h + ARROW_SNAP_PX) {
      consider('left', r.x, clamp(y, r.y, r.y + r.h), Math.abs(x - r.x))
      consider('right', r.x + r.w, clamp(y, r.y, r.y + r.h), Math.abs(x - (r.x + r.w)))
    }
    if (x >= r.x - ARROW_SNAP_PX && x <= r.x + r.w + ARROW_SNAP_PX) {
      consider('top', clamp(x, r.x, r.x + r.w), r.y, Math.abs(y - r.y))
      consider('bottom', clamp(x, r.x, r.x + r.w), r.y + r.h, Math.abs(y - (r.y + r.h)))
    }
  }
  return best
}

/**
 * Snap arrow tips (last point of paths with `marker-end`) onto the nearest rect
 * edge when they float within ARROW_SNAP_PX. Also re-point the previous vertex
 * when the final segment collapses, so the arrowhead direction stays correct.
 */
export function snapArrowEndpoints(svg: string): string {
  const rects: FlowRect[] = []
  const rectRe = /<rect\b([^>]*?)(\/>|>)/gi
  for (const m of svg.matchAll(rectRe)) {
    const attrs = m[1] ?? ''
    const x = attrNum(attrs, 'x', Number.NaN)
    const y = attrNum(attrs, 'y', Number.NaN)
    const w = attrNum(attrs, 'width', Number.NaN)
    const h = attrNum(attrs, 'height', Number.NaN)
    if ([x, y, w, h].some(n => !Number.isFinite(n)) || w <= 0 || h <= 0) continue
    rects.push({ x, y, w, h })
  }
  if (rects.length === 0) return svg

  return svg.replace(PATH_TAG_RE, (full, attrs: string, closing: string) => {
    if (!/\bmarker-end\s*=/i.test(attrs)) return full
    const d = attrStr(attrs, 'd')
    if (!d) return full
    const pts = pathPoints(d)
    if (pts.length < 2) return full
    const [ex, ey] = pts[pts.length - 1]!
    const [px, py] = pts[pts.length - 2]!
    const snap = nearestRectEdge(rects, ex, ey)
    if (!snap || snap.dist > ARROW_SNAP_PX || snap.dist < 0.5) return full

    let pxn = px
    let pyn = py
    if (Math.hypot(snap.tx - px, snap.ty - py) < 4) {
      // Final segment collapsed: move prev outside the snapped edge.
      if (snap.edge === 'top') {
        pxn = snap.tx
        pyn = snap.ty - 10
      } else if (snap.edge === 'bottom') {
        pxn = snap.tx
        pyn = snap.ty + 10
      } else if (snap.edge === 'left') {
        pxn = snap.tx - 10
        pyn = snap.ty
      } else {
        pxn = snap.tx + 10
        pyn = snap.ty
      }
    }
    const rebuilt = rebuildPathD(pts, pxn, pyn, snap.tx, snap.ty)
    const attrsClean = attrs.replace(/\bd\s*=\s*["'][\s\S]*?["']/i, ` d="${rebuilt}"`)
    return `<path${attrsClean}${closing}`
  })
}

/** Snap arrow endpoints first, then expand marker arrowheads into polygons. */
export function normalizeFlowArrows(svg: string): string {
  return expandSvgArrowMarkers(snapArrowEndpoints(svg))
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
  // Authors may ship responsive sizing (e.g. Mermaid emits width="100%" plus
  // `style="max-width: Npx"`). Forcing the raw viewBox pixel width then
  // overflows narrow containers — the diagram gets pinned to the top-left
  // corner and clipped on the right. Respect their responsive intent.
  // (Bare width="100%" without a max-width cap still means "crush to container"
  // — keep forcing the intrinsic pixel size for that case.)
  const maxWidthCap = root.style.maxWidth
  const hasMaxWidthCap =
    maxWidthCap !== undefined && maxWidthCap !== '' && maxWidthCap !== 'none'
  const responsive = root.getAttribute('width') === '100%' && hasMaxWidthCap
  if (responsive) {
    root.style.width = '100%'
    root.style.height = 'auto'
    // keep the author's max-width cap so wide diagrams scale down, not up
  } else if (size) {
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
