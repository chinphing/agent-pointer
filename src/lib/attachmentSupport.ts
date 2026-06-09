export const CHAT_ATTACHMENT_ACCEPT =
  'image/*,audio/*,video/*,application/pdf,text/*,.csv,.json,.md,.txt,.zip,' +
  '.doc,.docx,.xls,.xlsx,.ppt,.pptx,.mp4,.mov,.webm,.m4v,.avi,.mpeg,.mpg'

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

export function isSupportedChatAttachmentFile(file: Pick<File, 'name' | 'type'>): boolean {
  if (isVideoAttachmentFile(file)) return true
  if (file.type.startsWith('image/') || file.type.startsWith('audio/')) return true
  if (
    file.type.startsWith('text/') ||
    file.type === 'application/pdf' ||
    file.type === 'application/json' ||
    /\.(?:txt|md|json|csv|pdf|zip|doc|docx|xls|xlsx|ppt|pptx)$/i.test(file.name)
  ) {
    return true
  }
  return file.type === 'application/octet-stream' && file.name.trim().length > 0
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
