/** No MIME filter — user may attach from any folder and any file type. */
export const CHAT_ATTACHMENT_ACCEPT = '*/*'

/** Blob URLs expire after restart; only data/http(s) URLs are safe to reuse. */
export function isUsableAttachmentPreviewUrl(url: string | undefined | null): boolean {
  const u = url?.trim()
  if (!u) return false
  if (u.startsWith('blob:')) return false
  return u.startsWith('data:') || u.startsWith('http://') || u.startsWith('https://')
}

/** Align with IM inbound media cap (30 MB). */
export const COMPOSER_VIDEO_MAX_BYTES = 30 * 1024 * 1024

export function isSupportedChatAttachmentMimeType(mimeType: string | null | undefined): boolean {
  return typeof mimeType === 'string' && mimeType.trim().length > 0
}

export function isVideoAttachmentFile(file: Pick<File, 'name' | 'type'>): boolean {
  const mime = file.type.trim().toLowerCase()
  if (mime.startsWith('video/')) return true
  return /\.(?:avi|m4v|mov|mp4|mpeg|mpg|webm)$/i.test(file.name)
}

export function isSupportedChatAttachmentFile(file: Pick<File, 'name' | 'size' | 'type'>): boolean {
  const name = file.name.trim()
  if (!name) return false
  if (isVideoAttachmentFile(file) && file.size > COMPOSER_VIDEO_MAX_BYTES) return false
  return true
}

export function mediaKindFromFile(
  file: Pick<File, 'name' | 'type'>
): 'image' | 'document' | 'audio' | 'video' | 'file' {
  const mime = file.type.trim().toLowerCase()
  if (mime.startsWith('image/')) return 'image'
  if (mime.startsWith('audio/')) return 'audio'
  if (isVideoAttachmentFile(file)) return 'video'
  if (
    mime.startsWith('text/') ||
    mime === 'application/pdf' ||
    mime === 'application/json' ||
    /\.(?:txt|md|json|csv|pdf)$/i.test(file.name)
  ) {
    return 'document'
  }
  return 'file'
}

export function dataUrlToBase64(dataUrl: string): string {
  const comma = dataUrl.indexOf(',')
  if (comma < 0) return dataUrl.trim()
  return dataUrl.slice(comma + 1).trim()
}

export function composerVideoSizeError(file: Pick<File, 'name' | 'size' | 'type'>): string | null {
  if (!isVideoAttachmentFile(file)) return null
  if (file.size <= COMPOSER_VIDEO_MAX_BYTES) return null
  const limitMb = COMPOSER_VIDEO_MAX_BYTES / (1024 * 1024)
  return `视频 ${file.name} 超过 ${limitMb} MB 上限`
}
