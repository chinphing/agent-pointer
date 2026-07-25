export type CodeTokenKind = 'plain' | 'comment' | 'string' | 'number' | 'keyword'

export interface CodeToken {
  text: string
  kind: CodeTokenKind
}

const KEYWORDS = new Set([
  'async', 'await', 'break', 'case', 'class', 'const', 'continue', 'def', 'else', 'enum',
  'export', 'false', 'fn', 'for', 'from', 'function', 'if', 'impl', 'import', 'in', 'interface',
  'let', 'match', 'mod', 'new', 'null', 'pub', 'return', 'self', 'static', 'struct', 'super',
  'switch', 'this', 'throw', 'true', 'try', 'type', 'undefined', 'use', 'var', 'while', 'yield'
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
