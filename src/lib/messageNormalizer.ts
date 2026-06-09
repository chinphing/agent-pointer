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

export function attachmentsForMessageRender(message: ChatMessage): RenderableAttachment[] {
  const list = message.attachments ?? []
  return list.map(att => ({
    id: att.id,
    kind: att.kind,
    fileName: att.fileName,
    mimeType: att.mimeType,
    storageRelPath: att.storageRelPath,
    previewUrl:
      att.previewUrl ??
      getComposerAttachmentPreviewUrl(att as ComposerAttachment) ??
      undefined
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
