import type { ComposerAttachment } from '../types/chat'

type AttachmentPayload = {
  dataUrl?: string
  previewUrl?: string
  contentBase64?: string
}

const payloads = new Map<string, AttachmentPayload>()

function createObjectUrl(file: File): string | undefined {
  if (typeof URL === 'undefined' || typeof URL.createObjectURL !== 'function') {
    return undefined
  }
  return URL.createObjectURL(file)
}

function revokeObjectUrl(url: string | undefined): void {
  if (!url || typeof URL === 'undefined' || typeof URL.revokeObjectURL !== 'function') {
    return
  }
  URL.revokeObjectURL(url)
}

export function registerComposerAttachmentPayload(params: {
  attachment: ComposerAttachment
  dataUrl: string
  contentBase64: string
  file: File
}): ComposerAttachment {
  const previous = payloads.get(params.attachment.id)
  revokeObjectUrl(previous?.previewUrl)
  const objectUrl = createObjectUrl(params.file)
  const previewUrl = objectUrl ?? params.attachment.previewUrl
  payloads.set(params.attachment.id, {
    dataUrl: params.dataUrl,
    contentBase64: params.contentBase64,
    ...(previewUrl ? { previewUrl } : {})
  })
  return {
    ...params.attachment,
    ...(previewUrl ? { previewUrl } : {})
  }
}

export function getComposerAttachmentContentBase64(attachment: ComposerAttachment): string | null {
  if (attachment.contentBase64?.trim()) return attachment.contentBase64.trim()
  return payloads.get(attachment.id)?.contentBase64 ?? null
}

export function getComposerAttachmentPreviewUrl(attachment: ComposerAttachment): string | null {
  return (
    attachment.previewUrl ??
    payloads.get(attachment.id)?.previewUrl ??
    payloads.get(attachment.id)?.dataUrl ??
    null
  )
}

export function cloneComposerAttachmentsForSend(
  attachments: readonly ComposerAttachment[]
): ComposerAttachment[] {
  return attachments.map(att => {
    const b64 = getComposerAttachmentContentBase64(att)
    const { previewUrl: _preview, ...rest } = att
    return {
      ...rest,
      ...(b64 ? { contentBase64: b64 } : {})
    }
  })
}

export function releaseComposerAttachment(id: string): void {
  const previous = payloads.get(id)
  revokeObjectUrl(previous?.previewUrl)
  payloads.delete(id)
}

export function clearComposerAttachmentPayloads(): void {
  for (const id of payloads.keys()) {
    releaseComposerAttachment(id)
  }
}
