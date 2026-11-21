import { convertFileSrc } from '@tauri-apps/api/core'
import type { RenderableAttachment } from './messageNormalizer'
import { previewChatMedia, previewMediaRef } from './api'
import { resolveAttachmentAbsPath } from './attachmentLocalPath'
import { isUsableAttachmentPreviewUrl } from './attachmentSupport'
import {
  resolveVideoPreviewUrl,
  videoPreviewUrlFromLocalPath,
  videoPreviewUrlFromStorage
} from './chatMediaPreview'
import { isTauriRuntime } from './runtime'

/** Only images are eagerly preview-loaded in chat bubbles. */
export function shouldEagerLoadPreview(att: Pick<RenderableAttachment, 'kind'>): boolean {
  if (att.kind === 'image') return true
  return isTauriRuntime() && (att.kind === 'video' || att.kind === 'audio')
}

/** Web: non-image attachments show a download action instead of inline media. */
export function showsWebDownloadOnly(att: Pick<RenderableAttachment, 'kind'>): boolean {
  return !isTauriRuntime() && att.kind !== 'image'
}

export async function resolveImagePreviewDataUrl(
  att: RenderableAttachment,
  conversationId?: string | null
): Promise<string | null> {
  if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()

  let preview
  if (att.mediaRef && !att.storageRelPath) {
    preview = await previewMediaRef(att.mediaRef, conversationId)
  } else if (att.localAbsPath) {
    preview = await previewMediaRef(att.localAbsPath, conversationId)
  } else if (att.storageRelPath) {
    preview = await previewChatMedia(att.storageRelPath, conversationId)
  } else if (att.mediaRef) {
    preview = await previewMediaRef(att.mediaRef, conversationId)
  } else {
    return null
  }

  const mime =
    preview.mimeType && preview.mimeType !== 'application/octet-stream'
      ? preview.mimeType
      : att.mimeType || preview.mimeType || 'application/octet-stream'
  return `data:${mime};base64,${preview.dataBase64}`
}

/** Desktop playback URL. Local files use a file URL; sandbox media is read through the session. */
export async function resolveDesktopPlaybackUrl(
  att: RenderableAttachment,
  conversationId?: string | null
): Promise<string | null> {
  if (!isTauriRuntime()) return null

  if (att.kind === 'video') {
    const fromStorage = await resolveVideoPreviewUrl(att, conversationId)
    if (fromStorage) return fromStorage
    if (att.localAbsPath?.trim()) {
      return videoPreviewUrlFromLocalPath(att.localAbsPath)
    }
    const abs = await resolveAttachmentAbsPath(att, conversationId)
    if (abs) return convertFileSrc(abs)
    return null
  }

  if (att.kind === 'audio') {
    if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
    if (att.storageRelPath?.trim()) {
      const fromStorage = await videoPreviewUrlFromStorage(att.storageRelPath, conversationId)
      if (fromStorage) return fromStorage
    }
    const abs = await resolveAttachmentAbsPath(att, conversationId)
    if (abs) return convertFileSrc(abs)
    return null
  }

  return null
}

export async function resolveEagerPreviewUrl(
  att: RenderableAttachment,
  conversationId?: string | null
): Promise<string | null> {
  if (att.kind === 'image') return resolveImagePreviewDataUrl(att, conversationId)
  if (isTauriRuntime() && (att.kind === 'video' || att.kind === 'audio')) {
    return resolveDesktopPlaybackUrl(att, conversationId)
  }
  return null
}
