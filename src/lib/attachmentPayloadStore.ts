import type { ComposerAttachment } from '../types/chat'

type AttachmentPayload = {
  dataUrl?: string
  previewUrl?: string
  contentBase64?: string
  file?: File
}

const payloads = new Map<string, AttachmentPayload>()

/**
 * Attachments whose bytes are still held in memory. Read-only: `releaseComposerAttachment`
 * and `clearComposerAttachmentPayloads` remain the only writers (see `lib/residencyProbe.ts`).
 */
export function composerAttachmentPayloadCount(): number {
  return payloads.size
}

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

/** Register bytes payload (desktop path load / legacy). */
export function registerComposerAttachmentPayload(params: {
  attachment: ComposerAttachment
  dataUrl: string
  contentBase64: string
  file?: File
}): ComposerAttachment {
  const previous = payloads.get(params.attachment.id)
  revokeObjectUrl(previous?.previewUrl)
  const objectUrl = params.file ? createObjectUrl(params.file) : undefined
  const previewUrl = objectUrl ?? params.attachment.previewUrl
  payloads.set(params.attachment.id, {
    dataUrl: params.dataUrl,
    contentBase64: params.contentBase64,
    ...(params.file ? { file: params.file } : {}),
    ...(previewUrl ? { previewUrl } : {})
  })
  return {
    ...params.attachment,
    ...(previewUrl ? { previewUrl } : {})
  }
}

/** Optimistic chip: preview immediately from File; upload later. */
export function registerComposerAttachmentFile(params: {
  attachment: ComposerAttachment
  file: File
}): ComposerAttachment {
  const previous = payloads.get(params.attachment.id)
  revokeObjectUrl(previous?.previewUrl)
  const objectUrl = createObjectUrl(params.file)
  const previewUrl = objectUrl ?? params.attachment.previewUrl
  payloads.set(params.attachment.id, {
    file: params.file,
    ...(previewUrl ? { previewUrl } : {}),
    ...(previous?.dataUrl ? { dataUrl: previous.dataUrl } : {}),
    ...(previous?.contentBase64 ? { contentBase64: previous.contentBase64 } : {})
  })
  return {
    ...params.attachment,
    ...(previewUrl ? { previewUrl } : {})
  }
}

export function getComposerAttachmentFile(attachment: ComposerAttachment): File | null {
  return payloads.get(attachment.id)?.file ?? null
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

/** Stable data URL for optimistic message thumbnails (survives blob URL revoke). */
export function getComposerAttachmentDataUrl(attachment: ComposerAttachment): string | null {
  const payload = payloads.get(attachment.id)
  if (payload?.dataUrl) return payload.dataUrl
  const b64 = getComposerAttachmentContentBase64(attachment)
  if (!b64) return null
  const mime = attachment.mimeType || 'application/octet-stream'
  return `data:${mime};base64,${b64}`
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

/** Restore preview URLs from the in-memory payload store after draft reload. */
export function hydrateComposerAttachments(
  attachments: readonly ComposerAttachment[]
): ComposerAttachment[] {
  return attachments.map(att => {
    const previewUrl = getComposerAttachmentPreviewUrl(att)
    return previewUrl ? { ...att, previewUrl } : { ...att }
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
