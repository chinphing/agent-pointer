const MEDIA_PREFIX = 'MEDIA:'
const POINTER_SCHEME = 'pointer-media://'
const ATTACHMENT_ID_QUERY = '?attachmentId='

function trimTrailingPathPunct(s: string): string {
  return s.replace(/[,;)\]}.。、]+$/u, '').trim()
}

/** Split `path?attachmentId=…` (only this query key). */
export function splitAttachmentIdQuery(raw: string): { path: string; attachmentId: string | null } {
  const s = raw.trim()
  const i = s.lastIndexOf(ATTACHMENT_ID_QUERY)
  if (i < 0) return { path: s, attachmentId: null }
  const path = s.slice(0, i).trimEnd()
  if (!path) return { path: s, attachmentId: null }
  const idRaw = s.slice(i + ATTACHMENT_ID_QUERY.length).trim()
  const id = idRaw.split(/[\s&]/)[0]?.trim() || ''
  if (!id) return { path, attachmentId: null }
  return { path, attachmentId: id }
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
    return parseUnquotedMediaPath(rest.slice(1))
  }

  return parseUnquotedMediaPath(rest)
}

function parseUnquotedMediaPath(rest: string): string | null {
  const full = trimTrailingPathPunct(rest)
  if (full) return full

  const token = rest.split(/\s+/)[0]
  return token ? trimTrailingPathPunct(token) : null
}

function isMediaLine(trimmed: string): boolean {
  if (
    trimmed.length >= MEDIA_PREFIX.length
    && trimmed.slice(0, MEDIA_PREFIX.length).toUpperCase() === MEDIA_PREFIX
  ) {
    return true
  }
  return trimmed.toLowerCase().startsWith(POINTER_SCHEME)
}

/** Extract path references from assistant `MEDIA:` / bare `pointer-media://` lines. */
export function extractOutboundMediaPaths(text: string): string[] {
  if (!text.trim()) return []

  const paths: string[] = []
  const mediaRe = /\bMEDIA:/gi
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (
      trimmed.length >= MEDIA_PREFIX.length
      && trimmed.slice(0, MEDIA_PREFIX.length).toUpperCase() === MEDIA_PREFIX
    ) {
      const path = parseMediaPathAfterMarker(trimmed.slice(MEDIA_PREFIX.length))
      if (path) paths.push(splitAttachmentIdQuery(path).path)
      continue
    }
    if (trimmed.toLowerCase().startsWith(POINTER_SCHEME)) {
      paths.push(splitAttachmentIdQuery(trimmed).path)
      continue
    }
    mediaRe.lastIndex = 0
    let match: RegExpExecArray | null
    while ((match = mediaRe.exec(line)) !== null) {
      const after = line.slice(match.index + match[0].length)
      const path = parseMediaPathAfterMarker(after)
      if (path) {
        const clean = splitAttachmentIdQuery(path).path
        paths.push(clean)
        mediaRe.lastIndex = match.index + match[0].length + after.indexOf(path) + path.length
      }
    }
  }
  return paths
}

/** Strip `MEDIA:` / bare `pointer-media://` lines for bubble display (chips use attachments). */
export function stripOutboundMediaMarkers(text: string): string {
  const lines = text.split('\n')
  const kept: string[] = []
  for (const line of lines) {
    const trimmed = line.trim()
    if (isMediaLine(trimmed)) continue
    // Drop inline MEDIA:… segments on otherwise prose lines.
    if (/\bMEDIA:/i.test(line) || line.toLowerCase().includes(POINTER_SCHEME)) {
      let rest = line
      rest = rest.replace(/\bMEDIA:\s*(?:`[^`]+`|"[^"]+"|'[^']+'|[^\s]+)/gi, '')
      rest = rest.replace(/\bpointer-media:\/\/\S+/gi, '')
      if (!rest.trim()) continue
      kept.push(rest.replace(/[ \t]{2,}/g, ' ').trimEnd())
      continue
    }
    kept.push(line)
  }
  return kept.join('\n').replace(/\n{3,}/g, '\n\n').trim()
}
