export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function resolveWebApiBase(): string {
  const env = import.meta.env.VITE_WEB_API_BASE
  // Explicit empty string = same-origin (pointer-server integrated bundle).
  if (env !== undefined) {
    return String(env)
  }
  return 'http://127.0.0.1:8787'
}

export const WEB_API_BASE = resolveWebApiBase()
