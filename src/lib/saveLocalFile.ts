import { isTauriRuntime } from './runtime'

function dataUrlToBase64(dataUrl: string): string {
  const comma = dataUrl.indexOf(',')
  if (comma < 0) return dataUrl.trim()
  return dataUrl.slice(comma + 1).trim()
}

function triggerBrowserDownload(dataUrl: string, fileName: string) {
  const anchor = document.createElement('a')
  anchor.href = dataUrl
  anchor.download = fileName || 'download'
  anchor.rel = 'noopener'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}

export type SaveLocalFileResult = 'saved' | 'cancelled'

/**
 * Save bytes to disk. Desktop: native Save dialog + write.
 * Web: browser download via data URL / blob URL.
 */
export async function saveDataUrlAsFile(
  dataUrl: string,
  fileName: string,
  filters?: { name: string; extensions: string[] }[]
): Promise<SaveLocalFileResult> {
  const trimmed = dataUrl.trim()
  if (!trimmed) throw new Error('empty file content')

  if (!isTauriRuntime()) {
    triggerBrowserDownload(trimmed, fileName)
    return 'saved'
  }

  const { save } = await import('@tauri-apps/plugin-dialog')
  const { invoke } = await import('@tauri-apps/api/core')
  const path = await save({
    defaultPath: fileName,
    filters: filters?.length
      ? filters
      : [{ name: 'PNG', extensions: ['png'] }],
  })
  if (!path) {
    console.info('[saveLocalFile] user cancelled save dialog')
    return 'cancelled'
  }

  const base64 = dataUrlToBase64(trimmed)
  await invoke('save_bytes_to_path', {
    path,
    contentBase64: base64,
  })
  console.info('[saveLocalFile] saved', path)
  return 'saved'
}
