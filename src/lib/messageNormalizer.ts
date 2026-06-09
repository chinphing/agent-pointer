import type { ChatMessage, MediaAttachment, MediaAttachmentKind } from '../types/chat'
import { getComposerAttachmentPreviewUrl } from './attachmentPayloadStore'
import type { ComposerAttachment } from '../types/chat'
import { extractOutboundMediaPaths } from './outboundMedia'

export interface RenderableAttachment {
  id: string
  kind: MediaAttachment['kind']
  fileName: string
  mimeType: string
  previewUrl?: string
  storageRelPath?: string
  localAbsPath?: string
  /** Resolved preview key: storageRelPath or localAbsPath or pointer-media ref. */
  mediaRef?: string
}

function attachmentDataUrlFromBase64(att: MediaAttachment): string | undefined {
  const b64 = att.contentBase64?.trim()
  if (!b64) return undefined
  const mime = att.mimeType || 'application/octet-stream'
  return `data:${mime};base64,${b64}`
}

export function attachmentPreviewUrl(att: MediaAttachment): string | undefined {
  if (att.previewUrl) return att.previewUrl
  const fromStore = getComposerAttachmentPreviewUrl(att as ComposerAttachment)
  if (fromStore) return fromStore
  return attachmentDataUrlFromBase64(att)
}

function fileNameFromPath(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || 'attachment'
}

function kindFromFileName(fileName: string): MediaAttachmentKind {
  const ext = fileName.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) return 'image'
  if (['mp4', 'webm', 'mov', 'mkv'].includes(ext)) return 'video'
  if (['mp3', 'wav', 'm4a', 'aac', 'ogg', 'flac'].includes(ext)) return 'audio'
  if (['pdf', 'txt', 'md', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx'].includes(ext)) return 'document'
  return 'file'
}

function renderableFromMediaAttachment(att: MediaAttachment): RenderableAttachment {
  const mediaRef = att.localAbsPath ?? att.storageRelPath
  return {
    id: att.id,
    kind: att.kind,
    fileName: att.fileName,
    mimeType: att.mimeType,
    storageRelPath: att.storageRelPath,
    localAbsPath: att.localAbsPath,
    mediaRef,
    previewUrl: attachmentPreviewUrl(att)
  }
}

function isUserFilesystemPath(path: string): boolean {
  const t = path.trim()
  return t === '~' || t.startsWith('~/') || t.startsWith('~\\') ||
    path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path)
}

function renderableFromMediaPath(path: string, index: number): RenderableAttachment {
  const fileName = fileNameFromPath(path)
  const isFs = isUserFilesystemPath(path)
  const storageRelPath = !isFs && path.includes('/') ? path : undefined
  const localAbsPath = isFs ? path : undefined
  return {
    id: `reply-media-draft-${index}`,
    kind: kindFromFileName(fileName),
    fileName,
    mimeType: 'application/octet-stream',
    storageRelPath,
    localAbsPath,
    mediaRef: path
  }
}

export function attachmentsForMessageRender(message: ChatMessage): RenderableAttachment[] {
  const list = message.attachments ?? []
  if (list.length) return list.map(renderableFromMediaAttachment)
  if (message.role !== 'assistant') return []
  const paths = extractOutboundMediaPaths(message.rawContent?.trim() || message.content || '')
  return paths.map((path, i) => renderableFromMediaPath(path, i))
}

/** Assistant bubble: persisted attachments or streaming `MEDIA:` draft paths. */
export function assistantReplyMediaForRender(
  message: ChatMessage | undefined,
  streamingText?: string
): RenderableAttachment[] {
  if (message?.attachments?.length) {
    return message.attachments.map(renderableFromMediaAttachment)
  }
  const raw =
    streamingText?.trim() ||
    message?.rawContent?.trim() ||
    message?.content?.trim() ||
    ''
  const paths = extractOutboundMediaPaths(raw)
  return paths.map((path, i) => renderableFromMediaPath(path, i))
}

export function stripWireAttachmentFields(messages: ChatMessage[]): ChatMessage[] {
  return messages.map(msg => {
    if (!msg.attachments?.length) return msg
    return {
      ...msg,
      attachments: msg.attachments.map(({ contentBase64: _c, previewUrl: _p, ...rest }) => rest)
    }
  })
}
