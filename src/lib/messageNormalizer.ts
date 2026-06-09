import type { ChatMessage, MediaAttachment } from '../types/chat'
import { getComposerAttachmentPreviewUrl } from './attachmentPayloadStore'
import type { ComposerAttachment } from '../types/chat'

export interface RenderableAttachment {
  id: string
  kind: MediaAttachment['kind']
  fileName: string
  mimeType: string
  previewUrl?: string
  storageRelPath?: string
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

export function attachmentsForMessageRender(message: ChatMessage): RenderableAttachment[] {
  const list = message.attachments ?? []
  return list.map(att => ({
    id: att.id,
    kind: att.kind,
    fileName: att.fileName,
    mimeType: att.mimeType,
    storageRelPath: att.storageRelPath,
    previewUrl: attachmentPreviewUrl(att)
  }))
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
