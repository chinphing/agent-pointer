/**
 * Summarize an HTTP error response body into short text so nginx HTML pages or
 * large JSON blobs do not become the raw error message.
 *
 * Strategy:
 * 1. Empty body → `Request failed ({status})`
 * 2. HTML → extract `<title>`; otherwise strip tags and take first 200 chars
 * 3. JSON → take `error || message || detail` string fields
 * 4. Plain text → first 500 chars
 *
 * Keep status digits and keywords (e.g. "413") so retry.ts regexes still match.
 */
import { t } from '../i18n'

export async function summarizeErrorResponse(res: Response): Promise<string> {
  const status = res.status
  let text = ''
  try {
    text = await res.text()
  } catch {
    return t('http.requestFailed', { status })
  }

  if (!text || !text.trim()) {
    return t('http.requestFailed', { status })
  }

  const lower = text.trimStart().toLowerCase()

  const contentType = (res.headers.get('content-type') ?? '').toLowerCase()
  if (contentType.includes('text/html') || lower.startsWith('<!doctype') || lower.startsWith('<html')) {
    const titleMatch = text.match(/<title[^>]*>([\s\S]*?)<\/title>/i)
    if (titleMatch?.[1]?.trim()) {
      return `[${status}] ${titleMatch[1].trim().replace(/\s+/g, ' ').slice(0, 200)}`
    }
    const stripped = text.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim()
    if (stripped) {
      return `[${status}] ${stripped.slice(0, 200)}`
    }
    return t('http.requestFailed', { status })
  }

  if (lower.startsWith('{') || lower.startsWith('[')) {
    try {
      const parsed = JSON.parse(text)
      const detail = parsed?.error ?? parsed?.message ?? parsed?.detail
      if (typeof detail === 'string' && detail.trim()) {
        return `[${status}] ${detail.trim().slice(0, 500)}`
      }
    } catch {
      // fall through to plain-text clip
    }
  }

  const clipped = text.trim().slice(0, 500)
  return `[${status}] ${clipped}`
}
