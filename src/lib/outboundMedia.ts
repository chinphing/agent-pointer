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
    for (const match of line.matchAll(INLINE_MEDIA_RE)) {
      const path = match[1]?.trim()
      if (path) paths.push(path)
    }
  }
  return paths
}

/** @deprecated Resolved-aware stripping runs in pointer-core before content is persisted. */
export function stripOutboundMediaMarkers(text: string): string {
  return text.trim()
}
