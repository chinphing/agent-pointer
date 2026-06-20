import type { RenderableAttachment } from './messageNormalizer'
import { isUserFilesystemPath } from './attachmentLocalPath'
import { openPathWithDefaultApp, openChatMedia } from './api'
import { isTauriRuntime } from './runtime'

export function isOpenableFileAttachment(kind: string): boolean {
  return kind === 'document' || kind === 'file'
}

/** Open a file attachment with the OS default app (desktop) or download/open preview (web). */
export async function openAttachmentWithSystemDefault(att: RenderableAttachment): Promise<void> {
  if (isTauriRuntime()) {
    if (att.localAbsPath) {
      await openPathWithDefaultApp(att.localAbsPath)
      return
    }
    if (att.storageRelPath) {
      await openChatMedia(att.storageRelPath)
      return
    }
    const ref = att.mediaRef?.trim()
    if (ref && isUserFilesystemPath(ref)) {
      await openPathWithDefaultApp(ref)
      return
    }
    throw new Error('无法打开该附件')
  }

  const preview = att.previewUrl
  if (preview?.startsWith('data:') || preview?.startsWith('blob:')) {
    const anchor = document.createElement('a')
    anchor.href = preview
    anchor.download = att.fileName || 'attachment'
    anchor.rel = 'noopener'
    anchor.click()
    return
  }
  if (preview?.startsWith('http://') || preview?.startsWith('https://')) {
    window.open(preview, '_blank', 'noopener,noreferrer')
    return
  }
  throw new Error('网页端暂不支持打开该附件')
}
