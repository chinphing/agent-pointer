import { convertFileSrc } from '@tauri-apps/api/core'
import { getChatMediaLocalPath } from './api'
import { isTauriRuntime } from './runtime'

/** Desktop: streamable file URL for a saved conversation-media video. */
export async function videoPreviewUrlFromStorage(
  storageRelPath: string
): Promise<string | null> {
  if (!storageRelPath.trim() || !isTauriRuntime()) return null
  try {
    const abs = await getChatMediaLocalPath(storageRelPath)
    return convertFileSrc(abs)
  } catch (err) {
    console.warn('videoPreviewUrlFromStorage failed', err)
    return null
  }
}

/** Desktop: preview URL from an absolute local path (before backup is saved). */
export async function videoPreviewUrlFromLocalPath(
  localPath: string
): Promise<string | null> {
  if (!localPath.trim() || !isTauriRuntime()) return null
  try {
    return convertFileSrc(localPath.trim())
  } catch (err) {
    console.warn('videoPreviewUrlFromLocalPath failed', err)
    return null
  }
}
