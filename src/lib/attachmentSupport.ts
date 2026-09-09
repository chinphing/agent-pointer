/** No MIME filter — user may attach from any folder and any file type. */
export const CHAT_ATTACHMENT_ACCEPT = '*/*'

/** Blob URLs expire after restart; only data/http(s) URLs are safe to reuse. */
export function isUsableAttachmentPreviewUrl(url: string | undefined | null): boolean {
  const u = url?.trim()
  if (!u) return false
  if (u.startsWith('blob:')) return false
  return u.startsWith('data:') || u.startsWith('http://') || u.startsWith('https://')
}

/** Only http(s) preview URLs are safe to persist; data: URLs are stripped before disk. */
export function isPersistableAttachmentPreviewUrl(url: string | undefined | null): boolean {
  const u = url?.trim()
  return !!u && (u.startsWith('http://') || u.startsWith('https://'))
}

/** Align with host `COMPOSER_VIDEO_ADVISORY_BYTES` — compress via code after user confirms. */
export const COMPOSER_VIDEO_ADVISORY_BYTES = 500 * 1024 * 1024

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
  return !!name
}

export const DEFAULT_ATTACHMENT_UPLOAD_MAX_BYTES = 100 * 1024 * 1024

export function composerAttachmentUploadMaxBytes(raw?: number): number {
  const n = Number(raw)
  if (!Number.isFinite(n) || n <= 0) return DEFAULT_ATTACHMENT_UPLOAD_MAX_BYTES
  return Math.min(512 * 1024 * 1024, Math.max(1024 * 1024, Math.round(n)))
}

export function composerAttachmentTooLargeMessage(fileName: string, limitBytes: number): string {
  const limitMb = Math.max(1, Math.floor(limitBytes / (1024 * 1024)))
  return `「${fileName}」超过 ${limitMb} MB 上限`
}

export function mediaKindFromFile(
  file: Pick<File, 'name' | 'type'>
): 'image' | 'document' | 'audio' | 'video' | 'file' {
  const mime = file.type.trim().toLowerCase()
  if (
    (mime.startsWith('image/') && mime !== 'image/svg+xml') ||
    /\.(?:png|jpe?g|gif|webp|bmp|heic|heif|avif)$/i.test(file.name)
  ) {
    // SVG stays document (chat renders it as markup, not a raster chip).
    return 'image'
  }
  if (mime.startsWith('audio/') || /\.(?:mp3|wav|m4a|aac|ogg|flac)$/i.test(file.name)) {
    return 'audio'
  }
  if (isVideoAttachmentFile(file)) return 'video'
  if (
    mime.startsWith('text/') ||
    mime === 'application/pdf' ||
    mime === 'application/json' ||
    /\.(?:txt|md|json|csv|pdf|svg)$/i.test(file.name)
  ) {
    return 'document'
  }
  return 'file'
}

/** Best-effort MIME from filename when desktop path attach skips full-file read. */
export function mimeTypeFromFileName(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase() || ''
  switch (ext) {
    case 'png':
      return 'image/png'
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg'
    case 'gif':
      return 'image/gif'
    case 'webp':
      return 'image/webp'
    case 'bmp':
      return 'image/bmp'
    case 'pdf':
      return 'application/pdf'
    case 'txt':
      return 'text/plain'
    case 'md':
      return 'text/markdown'
    case 'json':
      return 'application/json'
    case 'mp3':
      return 'audio/mpeg'
    case 'wav':
      return 'audio/wav'
    case 'm4a':
      return 'audio/mp4'
    case 'svg':
      return 'image/svg+xml'
    default:
      return 'application/octet-stream'
  }
}

export function dataUrlToBase64(dataUrl: string): string {
  const comma = dataUrl.indexOf(',')
  if (comma < 0) return dataUrl.trim()
  return dataUrl.slice(comma + 1).trim()
}

export function isLargeComposerVideo(sizeBytes: number): boolean {
  return sizeBytes > COMPOSER_VIDEO_ADVISORY_BYTES
}

/** Confirm dialog when video exceeds advisory size — user must agree before code compress + upload. */
export function composerVideoCompressConfirmMessage(fileName: string, sizeBytes: number): string {
  const limitMb = COMPOSER_VIDEO_ADVISORY_BYTES / (1024 * 1024)
  const sizeMb = (sizeBytes / (1024 * 1024)).toFixed(1)
  return (
    `视频「${fileName}」约 ${sizeMb} MB。\n\n` +
    `超过 ${limitMb} MB 不能直接上传。Pointer 将启用压缩，把视频压缩到 ${limitMb} MB 以下后再上传；` +
    '视频的帧率和分辨率可能会降低。\n\n' +
    '是否继续？'
  )
}

/** Composer hint while compressing and uploading a large video. */
export function composerVideoCompressHint(fileName: string): string {
  return `正在压缩并上传「${fileName}」…`
}
