const MEDIA_PREFIX = 'MEDIA:'
const POINTER_SCHEME = 'pointer-media://'

function trimTrailingPathPunct(s: string): string {
  return s.replace(/[,;)\]}.。、]+$/u, '').trim()
}

/** Path that follows a `MEDIA:` marker (supports spaces; quoted or unquoted). */
function parseMediaPathAfterMarker(afterMarker: string): string | null {
  const rest = afterMarker.trimStart()
  if (!rest) return null

  const quote = rest[0]
  if (quote === '`' || quote === '"' || quote === "'") {
    const end = rest.indexOf(quote, 1)
    if (end > 0) {
      const inner = rest.slice(1, end).trim()
      if (inner) return inner
    }
    // Unclosed quote: treat remainder after the opening quote as an unquoted path.
    return parseUnquotedMediaPath(rest.slice(1))
  }

  return parseUnquotedMediaPath(rest)
}

function parseUnquotedMediaPath(rest: string): string | null {
  // Prefer remainder of line (macOS `Application Support`, etc.).
  const full = trimTrailingPathPunct(rest)
  if (full) return full

  const token = rest.split(/\s+/)[0]
  return token ? trimTrailingPathPunct(token) : null
}

/** Extract path references from assistant `MEDIA:` / bare `pointer-media://` lines. */
export function extractOutboundMediaPaths(text: string): string[] {
  if (!text.trim()) return []

  const paths: string[] = []
  const mediaRe = /\bMEDIA:/gi
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.length >= MEDIA_PREFIX.length
      && trimmed.slice(0, MEDIA_PREFIX.length).toUpperCase() === MEDIA_PREFIX) {
      const path = parseMediaPathAfterMarker(trimmed.slice(MEDIA_PREFIX.length))
      if (path) paths.push(path)
      continue
    }
    if (trimmed.toLowerCase().startsWith(POINTER_SCHEME)) {
      paths.push(trimmed)
      continue
    }
    mediaRe.lastIndex = 0
    let match: RegExpExecArray | null
    while ((match = mediaRe.exec(line)) !== null) {
      const after = line.slice(match.index + match[0].length)
      const path = parseMediaPathAfterMarker(after)
      if (path) {
        paths.push(path)
        // Advance past this path so a second MEDIA: on the same line can match.
        mediaRe.lastIndex = match.index + match[0].length + after.indexOf(path) + path.length
      }
    }
  }
  return paths
}

/** @deprecated Resolved-aware stripping runs in pointer-core before content is persisted. */
export function stripOutboundMediaMarkers(text: string): string {
  return text.trim()
}
