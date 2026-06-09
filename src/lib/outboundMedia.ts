const INLINE_MEDIA_RE = /\bMEDIA:\s*`?([^\s`\n]+)`?/gi
const MEDIA_PREFIX = 'MEDIA:'
const POINTER_SCHEME = 'pointer-media://'

/** Extract path references from assistant `MEDIA:` markers. */
export function extractOutboundMediaPaths(text: string): string[] {
  if (!text.trim()) return []

  const paths: string[] = []
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.toUpperCase().startsWith(MEDIA_PREFIX.toUpperCase())) {
      const path = trimmed.slice(MEDIA_PREFIX.length).trim()
      if (path) paths.push(path)
      continue
    }
    if (trimmed.startsWith(POINTER_SCHEME)) {
      const path = trimmed.slice(POINTER_SCHEME.length).trim()
      if (path) paths.push(path)
      continue
    }
    for (const match of line.matchAll(INLINE_MEDIA_RE)) {
      const path = match[1]?.trim()
      if (path) paths.push(path)
    }
  }
  return paths
}

/** Strip IM-only `MEDIA:` markers from assistant text shown in the App UI. */
export function stripOutboundMediaMarkers(text: string): string {
  if (!text.trim()) return text

  const kept: string[] = []

  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.toUpperCase().startsWith(MEDIA_PREFIX.toUpperCase())) continue
    if (trimmed.startsWith(POINTER_SCHEME)) continue
    const cleaned = line.replace(INLINE_MEDIA_RE, '')
    if (cleaned.trim()) kept.push(cleaned)
  }

  return kept.join('\n').trim()
}
