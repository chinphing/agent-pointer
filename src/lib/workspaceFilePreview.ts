export type CodeTokenKind = 'plain' | 'comment' | 'string' | 'number' | 'keyword'

export interface CodeToken {
  text: string
  kind: CodeTokenKind
}

const KEYWORDS = new Set([
  'async', 'await', 'break', 'case', 'class', 'const', 'continue', 'def', 'else', 'enum',
  'export', 'false', 'False', 'fn', 'for', 'from', 'function', 'if', 'impl', 'import', 'in',
  'interface', 'let', 'match', 'mod', 'new', 'None', 'null', 'pub', 'return', 'self', 'static',
  'struct', 'super', 'switch', 'this', 'throw', 'true', 'True', 'try', 'type', 'undefined',
  'use', 'var', 'while', 'yield'
])

const TOKEN_PATTERN = /(\/\/.*$|#.*$|<!--.*?-->|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_$][\w$]*\b)/g

export function tokenizeCodeLine(line: string): CodeToken[] {
  const tokens: CodeToken[] = []
  let cursor = 0
  for (const match of line.matchAll(TOKEN_PATTERN)) {
    const index = match.index ?? 0
    if (index > cursor) tokens.push({ text: line.slice(cursor, index), kind: 'plain' })
    const text = match[0]
    let kind: CodeTokenKind = 'plain'
    if (text.startsWith('//') || text.startsWith('#') || text.startsWith('<!--')) kind = 'comment'
    else if (/^["'`]/.test(text)) kind = 'string'
    else if (/^\d/.test(text)) kind = 'number'
    else if (KEYWORDS.has(text)) kind = 'keyword'
    tokens.push({ text, kind })
    cursor = index + text.length
    if (kind === 'comment') break
  }
  if (cursor < line.length) tokens.push({ text: line.slice(cursor), kind: 'plain' })
  return tokens.length ? tokens : [{ text: line || '\u00A0', kind: 'plain' }]
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/**
 * Highlight a fenced code body for Markdown `v-html` (chat + .md preview).
 * Same token kinds / colors as workspace source preview — not a full grammar.
 */
export function highlightCodeFenceHtml(code: string): string {
  const endsWithNl = code.endsWith('\n')
  const body = endsWithNl ? code.slice(0, -1) : code
  const html = body
    .split('\n')
    .map(line => {
      return tokenizeCodeLine(line)
        .map(token => {
          const raw = token.text === '\u00A0' && line === '' ? '' : token.text
          const escaped = escapeHtml(raw)
          if (!escaped || token.kind === 'plain') return escaped
          return `<span class="token-${token.kind}">${escaped}</span>`
        })
        .join('')
    })
    .join('\n')
  return endsWithNl ? `${html}\n` : html
}

export type FilePreviewSearchMatch = {
  lineIndex: number
  start: number
  end: number
}

export type FilePreviewSearchPart = {
  text: string
  matchIndex?: number
}

/** Case-insensitive occurrence matches for workspace text preview find. */
export function findFilePreviewMatches(content: string, query: string): FilePreviewSearchMatch[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return []

  const matches: FilePreviewSearchMatch[] = []
  const lines = content.split('\n')
  lines.forEach((line, lineIndex) => {
    const haystack = line.toLocaleLowerCase()
    let from = 0
    while (from <= haystack.length - needle.length) {
      const index = haystack.indexOf(needle, from)
      if (index < 0) break
      matches.push({ lineIndex, start: index, end: index + needle.length })
      from = index + needle.length
    }
  })
  return matches
}

/** Split one source line into plain/match segments for Vue rendering. */
export function filePreviewSearchParts(
  line: string,
  lineIndex: number,
  matches: FilePreviewSearchMatch[]
): FilePreviewSearchPart[] {
  const lineMatches = matches
    .map((match, matchIndex) => ({ match, matchIndex }))
    .filter(({ match }) => match.lineIndex === lineIndex)
  if (!lineMatches.length) return [{ text: line || '\u00A0' }]

  const parts: FilePreviewSearchPart[] = []
  let cursor = 0
  for (const { match, matchIndex } of lineMatches) {
    if (match.start > cursor) parts.push({ text: line.slice(cursor, match.start) })
    parts.push({ text: line.slice(match.start, match.end), matchIndex })
    cursor = match.end
  }
  if (cursor < line.length) parts.push({ text: line.slice(cursor) })
  return parts.length ? parts : [{ text: line || '\u00A0' }]
}
