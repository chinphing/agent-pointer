export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export const WEB_API_BASE = import.meta.env.VITE_WEB_API_BASE || 'http://127.0.0.1:8787'
