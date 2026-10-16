/**
 * KaTeX math for chat Markdown — inline `$…$` and display `$$…$$`.
 *
 * Delimiters follow GitHub's rules: the opening `$` may not be followed by
 * whitespace, the closing `$` may not be preceded by whitespace nor followed by
 * a digit, and inline math never spans a line break. `\$` stays a literal
 * dollar. Neither form is matched inside code spans or fenced code: marked
 * checks inline extensions before `codespan` (so a span always starts with a
 * backtick here) and consumes whole fenced blocks before block extensions run
 * again.
 *
 * An **unclosed** `$` (or `$$`) matches nothing and therefore falls through to
 * plain text — the streaming case must never swallow the rest of the reply.
 */

import katex from 'katex'
import type { TokenizerAndRendererExtension, Tokens } from 'marked'

export type MathDisplay = 'inline' | 'display'

/** Cap on a single formula's source; anything larger stays plain text. */
export const MAX_MATH_SOURCE_CHARS = 20_000
/** Cap on memoized KaTeX renders (streaming re-parses repeat the same formulas). */
export const MATH_HTML_CACHE_MAX_ENTRIES = 500

/** Token types produced by the extensions below (also their `name`). */
export const INLINE_MATH_TOKEN_TYPE = 'mathInline'
export const BLOCK_MATH_TOKEN_TYPE = 'mathBlock'

const DOLLAR = 0x24

export type MathMatch = { raw: string; source: string }

function isDigit(ch: string | undefined): boolean {
  if (ch === undefined) return false
  const code = ch.charCodeAt(0)
  return code >= 0x30 && code <= 0x39
}

function isWhitespace(ch: string | undefined): boolean {
  return ch !== undefined && /\s/.test(ch)
}

function isUsableSource(source: string): boolean {
  return source.length > 0 && source.length <= MAX_MATH_SOURCE_CHARS
}

/**
 * Match a GitHub-style inline `$…$` at the start of `src`.
 * Returns `null` when no complete formula opens here.
 */
export function findInlineMath(src: string): MathMatch | null {
  if (src.charCodeAt(0) !== DOLLAR || src.charCodeAt(1) === DOLLAR) return null
  const afterOpen = src[1]
  if (afterOpen === undefined || isWhitespace(afterOpen)) return null
  for (let i = 1; i < src.length; i++) {
    const ch = src[i]!
    if (ch === '\\') {
      i += 1 // `\$` is a literal dollar, not a delimiter
      continue
    }
    if (ch === '\n') return null // inline math never spans a line break
    if (ch !== '$') continue
    if (isWhitespace(src[i - 1])) continue
    if (isDigit(src[i + 1])) continue
    const source = src.slice(1, i)
    if (!isUsableSource(source)) continue
    return { raw: src.slice(0, i + 1), source }
  }
  return null
}

/**
 * Match `$$…$$` that stays on one line — the form a paragraph (or table cell)
 * swallows before the block tokenizer can see it. Same whitespace rules.
 */
export function findInlineDisplayMath(src: string): MathMatch | null {
  if (src.charCodeAt(0) !== DOLLAR || src.charCodeAt(1) !== DOLLAR) return null
  const close = src.indexOf('$$', 2)
  if (close < 0) return null
  const source = src.slice(2, close)
  if (source.includes('\n')) return null
  if (!isUsableSource(source)) return null
  if (isWhitespace(source[0]) || isWhitespace(source[source.length - 1])) return null
  return { raw: src.slice(0, close + 2), source }
}

const BLOCK_MATH_OPEN_RE = /^ {0,3}\$\$/

/**
 * Match a display `$$…$$` block at the start of `src`: the opening `$$` sits at
 * a line start (≤3 spaces of indent) and the closing `$$` ends a line. Requires
 * the closing delimiter, so a half-streamed block stays plain text.
 */
export function findBlockMath(src: string): MathMatch | null {
  const open = BLOCK_MATH_OPEN_RE.exec(src)
  if (!open) return null
  const contentStart = open[0].length

  const firstLineEnd = src.indexOf('\n', contentStart)
  const firstLine = (firstLineEnd < 0 ? src.slice(contentStart) : src.slice(contentStart, firstLineEnd))
    .replace(/[ \t]+$/, '')
  if (firstLine.length > 2 && firstLine.endsWith('$$')) {
    const source = firstLine.slice(0, -2).trim()
    if (!isUsableSource(source)) return null
    return { raw: src.slice(0, contentStart + firstLine.length), source }
  }

  let lineStart = firstLineEnd
  while (lineStart >= 0) {
    const lineEnd = src.indexOf('\n', lineStart + 1)
    const line = (lineEnd < 0 ? src.slice(lineStart + 1) : src.slice(lineStart + 1, lineEnd))
      .replace(/[ \t]+$/, '')
    if (line.endsWith('$$')) {
      const closeEnd = lineStart + 1 + line.length
      const source = src.slice(contentStart, closeEnd - 2).trim()
      if (!isUsableSource(source)) return null
      return { raw: src.slice(0, closeEnd), source }
    }
    lineStart = lineEnd
  }
  return null
}

/**
 * Index of the first `$$` in `src` that really opens a block. Used as the
 * extension `start`, which clips the paragraph token so `$$` on its own line is
 * not swallowed into the preceding prose.
 */
export function findBlockMathStart(src: string): number {
  let from = 0
  for (;;) {
    const at = src.indexOf('$$', from)
    if (at < 0) return -1
    const lineStart = src.lastIndexOf('\n', at - 1) + 1
    const indent = src.slice(lineStart, at)
    if (indent.length <= 3 && !/\S/.test(indent) && findBlockMath(src.slice(at))) return at
    from = at + 2
  }
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

const mathHtmlCache = new Map<string, string>()

/**
 * Entries currently held by the formula memo. Read-only: it exists so the HUD
 * can report growth (see `lib/residencyProbe.ts`).
 */
export function mathHtmlCacheSize(): number {
  return mathHtmlCache.size
}

/**
 * KaTeX HTML+MathML for one formula, memoized by (display, source) — a streaming
 * reply re-parses on every tick and would otherwise re-render every formula.
 *
 * `throwOnError: false` keeps a bad formula as inline error text, `trust: false`
 * disables `\href` / `\url` targets, and `htmlAndMathml` keeps the accessible
 * MathML alongside the visual HTML.
 */
export function renderMathHtml(source: string, display: MathDisplay): string {
  const key = (display === 'display' ? 'd' : 'i') + '\u0000' + source
  const cached = mathHtmlCache.get(key)
  if (cached !== undefined) return cached

  let body: string
  try {
    body = katex.renderToString(source, {
      displayMode: display === 'display',
      throwOnError: false,
      output: 'htmlAndMathml',
      trust: false,
      strict: false,
      // Theme-aware error text (KaTeX writes this into the inline style).
      errorColor: 'hsl(var(--danger))',
    })
  } catch (err) {
    // `throwOnError: false` covers parse errors; this guards renderer bugs so
    // one bad formula can never break the whole message.
    console.warn('[markdownMath] katex render failed', err)
    body = `<span class="katex-error">${escapeHtml(source)}</span>`
  }

  if (mathHtmlCache.size >= MATH_HTML_CACHE_MAX_ENTRIES) {
    const oldest = mathHtmlCache.keys().next().value
    if (oldest !== undefined) mathHtmlCache.delete(oldest)
  }
  mathHtmlCache.set(key, body)
  return body
}

/** Stable wrapper classes so tests and CSS can target rendered formulas. */
export function mathHtml(source: string, display: MathDisplay): string {
  const cls = display === 'display' ? 'md-math md-math--block' : 'md-math'
  return `<span class="${cls}">${renderMathHtml(source, display)}</span>`
}

type MathToken = Tokens.Generic & { text: string; display: boolean }

export const inlineMathExtension: TokenizerAndRendererExtension = {
  name: INLINE_MATH_TOKEN_TYPE,
  level: 'inline',
  // Inline extensions are consulted at the current position, and marked clips
  // the plain-text run at `start` so a mid-paragraph `$` still gets a look.
  start(src: string): number {
    return src.indexOf('$')
  },
  tokenizer(src: string): Tokens.Generic | undefined {
    const display = src.charCodeAt(1) === DOLLAR
    const match = display ? findInlineDisplayMath(src) : findInlineMath(src)
    if (!match) return undefined
    return { type: INLINE_MATH_TOKEN_TYPE, raw: match.raw, text: match.source, display }
  },
  renderer(token: Tokens.Generic): string {
    const math = token as MathToken
    return mathHtml(math.text, math.display ? 'display' : 'inline')
  },
}

export const blockMathExtension: TokenizerAndRendererExtension = {
  name: BLOCK_MATH_TOKEN_TYPE,
  level: 'block',
  start: findBlockMathStart,
  tokenizer(src: string): Tokens.Generic | undefined {
    const match = findBlockMath(src)
    if (!match) return undefined
    return { type: BLOCK_MATH_TOKEN_TYPE, raw: match.raw, text: match.source, display: true }
  },
  renderer(token: Tokens.Generic): string {
    const math = token as MathToken
    return mathHtml(math.text, 'display') + '\n'
  },
}

/** Registered by `markdownConfig.ts` in this order (display before inline). */
export const mathExtensions: TokenizerAndRendererExtension[] = [
  blockMathExtension,
  inlineMathExtension,
]
