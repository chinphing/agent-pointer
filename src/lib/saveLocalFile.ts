import { isTauriRuntime } from './runtime'

export type SaveLocalFileResult = 'saved' | 'cancelled'

function triggerBrowserDownload(dataUrl: string, fileName: string) {
  const anchor = document.createElement('a')
  anchor.href = dataUrl
  anchor.download = fileName || 'download'
  anchor.rel = 'noopener'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64)
  const out = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
  return out
}

/**
 * Decode a data URL into raw file bytes.
 * Supports `;base64,` (PNG charts) and URI-encoded payloads
 * (`data:image/svg+xml;charset=utf-8,…` from SVG export).
 */
export function dataUrlToBytes(dataUrl: string): Uint8Array {
  const trimmed = dataUrl.trim()
  const comma = trimmed.indexOf(',')
  if (comma < 0) {
    throw new Error('invalid data url: missing comma')
  }
  const meta = trimmed.slice(0, comma)
  if (!/^data:/i.test(meta)) {
    throw new Error('invalid data url: missing data: scheme')
  }
  const payload = trimmed.slice(comma + 1)
  if (!payload) {
    throw new Error('invalid data url: empty payload')
  }
  if (/;base64/i.test(meta)) {
    return base64ToBytes(payload.trim())
  }
  // Percent-encoded text (SVG export) or raw ASCII.
  try {
    return new TextEncoder().encode(decodeURIComponent(payload))
  } catch (err) {
    console.warn('[saveLocalFile] decodeURIComponent failed; using raw payload', err)
    return new TextEncoder().encode(payload)
  }
}

/** Bytes ready for `save_bytes_to_path` (always standard base64). */
export function dataUrlToContentBase64(dataUrl: string): string {
  return bytesToBase64(dataUrlToBytes(dataUrl))
}

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

  const contentBase64 = dataUrlToContentBase64(trimmed)
  await invoke('save_bytes_to_path', {
    path,
    contentBase64,
  })
  console.info('[saveLocalFile] saved', path)
  return 'saved'
}
