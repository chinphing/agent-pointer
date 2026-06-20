import type { RenderableAttachment } from './messageNormalizer'
import { getChatMediaLocalPath } from './tauri'
import { isTauriRuntime } from './runtime'

export function isUserFilesystemPath(path: string): boolean {
  const t = path.trim()
  if (t === '~' || t.startsWith('~/') || t.startsWith('~\\')) return true
  if (/^[A-Za-z]:[\\/]/.test(t)) return true
  return path.startsWith('/')
}

/** Resolve a chat attachment to an absolute on-disk path (desktop only). */
export async function resolveAttachmentAbsPath(
  att: Pick<RenderableAttachment, 'localAbsPath' | 'storageRelPath' | 'mediaRef'>
): Promise<string | null> {
  const local = att.localAbsPath?.trim()
  if (local && isUserFilesystemPath(local)) return local

  const rel = att.storageRelPath?.trim()
  if (rel && isTauriRuntime()) {
    try {
      return await getChatMediaLocalPath(rel)
    } catch (err) {
      console.warn('resolveAttachmentAbsPath: storage rel failed', rel, err)
    }
  }

  const ref = att.mediaRef?.trim()
  if (ref && isUserFilesystemPath(ref)) return ref

  return null
}

export async function prefetchAttachmentAbsPaths(
  attachments: RenderableAttachment[]
): Promise<Record<string, string>> {
  const out: Record<string, string> = {}
  await Promise.all(
    attachments.map(async att => {
      const abs = await resolveAttachmentAbsPath(att)
      if (abs) out[att.id] = abs
    })
  )
  return out
}
