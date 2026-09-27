import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { t } from '../i18n'
import * as api from '../lib/api'
import type { AuthMode } from '../lib/api'
import {
  loginRequiredMessage,
  mapLoginGateError,
  type LoginRequiredPurpose
} from '../lib/platformAuthMessages'
import { setTurnExpandStorageScope } from '../lib/turnExpandState'
import { isTauriRuntime } from '../lib/runtime'
import { useSettingsStore } from './settings'

export type { LoginRequiredPurpose }
export { loginRequiredMessage, mapLoginGateError }

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
  /** Stable platform / local / SSO user id for per-user client prefs. */
  userId?: string | null
  isPlatformAdmin?: boolean
  includedTokens?: number
  consumedTokens?: number
  tokenQuotaExhausted?: boolean
}

/** Prefer stable `http_status=NNN`, then common HTTP shapes in backend errors. */
export function extractPlatformAuthHttpStatus(message: string): number | null {
  const tagged = message.match(/\bhttp_status=(\d{3})\b/i)
  if (tagged) {
    const code = Number(tagged[1])
    return Number.isFinite(code) ? code : null
  }
  const exchange = message.match(/token exchange failed\s*\((\d{3})\b/i)
  if (exchange) {
    const code = Number(exchange[1])
    return Number.isFinite(code) ? code : null
  }
  const httpWord = message.match(/\bHTTP[ :](\d{3})\b/i)
  if (httpWord) {
    const code = Number(httpWord[1])
    return Number.isFinite(code) ? code : null
  }
  return null
}

function isAuthFailureHttpStatus(status: number): boolean {
  return status === 401 || status === 403
}

function isTransientHttpStatus(status: number): boolean {
  return status === 408 || status === 429 || (status >= 500 && status <= 599)
}

/** True when refresh/token failed due to transport / retryable HTTP — not a revoked session. */
export function isPlatformAuthTransientError(message: string): boolean {
  const msg = message.toLowerCase()
  if (msg.includes('invalid_refresh_token')) return false
  // Localized session-expired copy (zh / en) is not a transport blip.
  if (msg.includes('登录已失效') || msg.includes('session expired')) return false
  if (msg.includes('platform_login_required') || msg.includes('local_login_required')) return false
  if (msg.includes('platform_token_expired')) return false

  // Prefer HTTP status when the backend tagged the error (`http_status=NNN`).
  const status = extractPlatformAuthHttpStatus(message)
  if (status != null) {
    if (isAuthFailureHttpStatus(status)) return false
    return isTransientHttpStatus(status)
  }

  // No parseable status: only treat clear transport failures as transient.
  // Do not scan for bare "401" in prose — that misfires on proxy/HTML noise.
  return (
    msg.includes('网络异常') ||
    msg.includes('暂时无法') ||
    msg.includes('network error') ||
    msg.includes('cannot verify session') ||
    msg.includes('token request failed') ||
    msg.includes('error sending request') ||
    msg.includes('error trying to connect') ||
    msg.includes('connection reset') ||
    msg.includes('connection refused') ||
    msg.includes('timed out') ||
    msg.includes('timeout') ||
    msg.includes('dns error') ||
    msg.includes('network unreachable') ||
    msg.includes('连接')
  )
}

export type RequireSessionOptions = {
  purpose?: LoginRequiredPurpose
  /**
   * When refresh fails transiently but UI still has a prior logged-in session:
   * - `error` (default): throw the network/auth message (chat send)
   * - `allow`: proceed with the prior session (Composer attach)
   */
  onTransient?: 'error' | 'allow'
  /**
   * Skip a network refresh when a successful refresh completed within this many
   * milliseconds and access still has headroom. Composer multi-attach uses this
   * so each file does not pay a full refresh + settings reload.
   */
  maxAgeMs?: number
}

/** Access token expiry is unix seconds from the host. */
function accessHasHeadroom(session: PlatformSessionView, minRemainMs = 120_000): boolean {
  const exp = session.expires_at
  if (exp == null || !Number.isFinite(exp)) return true
  const expMs = exp > 1e12 ? exp : exp * 1000
  return expMs - Date.now() > minRemainMs
}

export function formatPlatformAuthError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('oauth callback timeout')) return t('auth.loginTimeout')
  if (msg.includes('platform_login_cancelled')) return t('auth.loginCancelled')
  if (msg.includes('本机回环') || msg.toLowerCase().includes('loopback')) {
    return t('auth.loopbackUnavailable')
  }
  const status = extractPlatformAuthHttpStatus(msg)
  if (
    msg.includes('invalid_refresh_token') ||
    msg.includes('platform_token_expired') ||
    (status != null && isAuthFailureHttpStatus(status))
  ) {
    return t('auth.sessionExpired')
  }
  if (msg.includes('server_access_denied')) return t('auth.serverAccessDenied')
  if (msg.includes('invalid_captcha')) return t('auth.invalidCaptcha')
  if (msg.includes('invalid_credentials')) return t('auth.invalidCredentials')
  if (msg.includes('local_login_required')) return t('auth.loginRequired')
  if (msg.includes('Plugin not found') || msg.includes('not allowed')) {
    return t('auth.cloudInjected')
  }
  if (isPlatformAuthTransientError(msg)) {
    return t('auth.networkVerifyFailed')
  }
  return msg
}

/** Desktop: refresh from auth.dat. Web: read session; after OAuth redirect, refresh to validate cookie. */
async function resolvePlatformSession(): Promise<PlatformSessionView> {
  if (isTauriRuntime()) {
    return api.refreshPlatformSession()
  }
  if (typeof window !== 'undefined') {
    const url = new URL(window.location.href)
    if (url.searchParams.get('platform_login') === 'success') {
      return api.refreshPlatformSession()
    }
  }
  return api.getPlatformSession()
}

/**
 * Web only: after OAuth / SSO redirects, surface errors and strip query so a
 * refresh does not re-trigger the toast. Tauri runtime is a no-op.
 */
function consumeOAuthRedirectQuery(): string | null {
  if (isTauriRuntime()) return null
  if (typeof window === 'undefined') return null
  const url = new URL(window.location.href)
  const successFlag = url.searchParams.get('platform_login')
  const errorMsg = url.searchParams.get('platform_login_error')
  const ssoError = url.searchParams.get('sso_error')
  if (successFlag !== 'success' && !errorMsg && !ssoError) return null
  url.searchParams.delete('platform_login')
  url.searchParams.delete('platform_login_error')
  url.searchParams.delete('sso_error')
  window.history.replaceState({}, '', url.toString())
  if (errorMsg === 'server_access_denied') {
    return t('auth.serverAccessDenied')
  }
  if (ssoError) {
    if (ssoError === 'not_configured') return t('auth.ssoNotConfigured')
    if (ssoError === 'not_standalone') return t('auth.ssoNotStandalone')
    if (ssoError === 'missing') return t('auth.ssoMissing')
    return t('auth.ssoFailed')
  }
  return errorMsg ? decodeURIComponent(errorMsg) : null
}

export const usePlatformAuthStore = defineStore('platformAuth', () => {
  const session = ref<PlatformSessionView>({ logged_in: false })
  const loading = ref(false)
  const error = ref<string | null>(null)
  const authMode = ref<AuthMode>('platform')
  /** Wall-clock of last successful ensureFreshSession network round-trip. */
  let lastFreshAtMs = 0
  /** Coalesce concurrent refresh calls (multi-file attach). */
  let freshInflight: Promise<PlatformSessionView> | null = null

  const isPlatformAdmin = computed(() => session.value.isPlatformAdmin === true)
  const isStandalone = computed(() => authMode.value === 'standalone')
  const tokenQuotaExhausted = computed(
    () => session.value.logged_in && session.value.tokenQuotaExhausted === true
  )

  watch(
    () => session.value.logged_in,
    loggedIn => {
      if (loggedIn) error.value = null
    }
  )

  // Isolate turn expand/collapse prefs per platform user (localStorage buckets).
  watch(
    () => (session.value.logged_in ? session.value.userId?.trim() || null : null),
    userId => {
      setTurnExpandStorageScope(userId)
    },
    { immediate: true }
  )

  async function loadAuthMode() {
    if (isTauriRuntime()) {
      authMode.value = 'platform'
      return
    }
    try {
      authMode.value = await api.getAuthMode()
    } catch (e) {
      console.warn('platformAuth: getAuthMode failed', e)
      authMode.value = 'platform'
    }
  }

  async function load() {
    loading.value = true
    error.value = null
    try {
      await loadAuthMode()
      session.value = await resolvePlatformSession()
      lastFreshAtMs = Date.now()
      const settings = useSettingsStore()
      await settings.load()
      const redirectError = consumeOAuthRedirectQuery()
      if (redirectError) {
        error.value = redirectError
      }
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e)
      error.value = formatPlatformAuthError(e)
      // Network blips must not wipe a still-valid UI session as "logged out".
      if (!isPlatformAuthTransientError(raw)) {
        session.value = { logged_in: false }
      } else {
        console.warn('[platformAuth] load failed transiently; keeping prior session', raw)
      }
    } finally {
      loading.value = false
    }
  }

  async function ensureFreshSession(options?: {
    maxAgeMs?: number
  }): Promise<PlatformSessionView> {
    const maxAgeMs = options?.maxAgeMs ?? 0
    if (
      maxAgeMs > 0 &&
      session.value.logged_in &&
      lastFreshAtMs > 0 &&
      Date.now() - lastFreshAtMs < maxAgeMs &&
      accessHasHeadroom(session.value)
    ) {
      return session.value
    }
    if (freshInflight) return freshInflight

    freshInflight = (async () => {
      try {
        if (isTauriRuntime()) {
          session.value = await api.refreshPlatformSession()
          if (session.value.logged_in) {
            error.value = null
            const settings = useSettingsStore()
            await settings.load()
          }
        } else {
          session.value = await api.getPlatformSession()
          if (session.value.logged_in) error.value = null
        }
        lastFreshAtMs = Date.now()
        return session.value
      } catch (e) {
        if (loading.value) throw e
        const raw = e instanceof Error ? e.message : String(e)
        error.value = formatPlatformAuthError(e)
        const status = extractPlatformAuthHttpStatus(raw)
        if (
          raw.includes('invalid_refresh_token') ||
          raw.includes('platform_token_expired') ||
          (status != null && isAuthFailureHttpStatus(status))
        ) {
          session.value = { logged_in: false }
          lastFreshAtMs = 0
        }
        throw new Error(error.value || t('auth.refreshFailed'))
      } finally {
        freshInflight = null
      }
    })()
    return freshInflight
  }

  /**
   * Single entry for “must be logged in before this action”.
   * Refreshes session first; throws a stable user-facing message when not logged in.
   */
  async function requireSession(
    options: RequireSessionOptions = {}
  ): Promise<PlatformSessionView> {
    const purpose = options.purpose ?? 'default'
    const onTransient = options.onTransient ?? 'error'
    const hint = loginRequiredMessage(isStandalone.value, purpose)
    try {
      await ensureFreshSession({ maxAgeMs: options.maxAgeMs })
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e)
      if (onTransient === 'allow' && isPlatformAuthTransientError(raw) && session.value.logged_in) {
        console.warn('[platformAuth] requireSession: transient refresh; keeping session', raw)
        return session.value
      }
      if (!session.value.logged_in) {
        throw new Error(
          raw && isPlatformAuthTransientError(raw) ? raw : error.value || hint
        )
      }
      throw e instanceof Error ? e : new Error(String(e))
    }
    if (!session.value.logged_in) {
      throw new Error(error.value || hint)
    }
    return session.value
  }

  function loginHint(purpose: LoginRequiredPurpose = 'default'): string {
    return loginRequiredMessage(isStandalone.value, purpose)
  }

  /** Prefer standard login copy for gate errors; otherwise return the original message. */
  function formatLoginGateError(
    err: unknown,
    purpose: LoginRequiredPurpose = 'default'
  ): string {
    const raw = err instanceof Error ? err.message : String(err)
    return mapLoginGateError(raw, isStandalone.value, purpose) || raw
  }

  async function login() {
    loading.value = true
    error.value = null
    try {
      const view = await api.openPlatformLogin()
      if (isTauriRuntime()) {
        // Unblock UI as soon as oauth exchange finished; refresh credentials in background.
        session.value = view ?? { logged_in: true }
        error.value = null
        loading.value = false
        const settings = useSettingsStore()
        void (async () => {
          try {
            session.value = await api.refreshPlatformSession()
            await settings.load()
            window.location.reload()
          } catch (e) {
            console.warn('platformAuth: post-login refresh failed', e)
          }
        })()
        return
      }
      // Web: openPlatformLogin redirected the browser away. The remaining
      // lines never run; the SPA reloads at /?platform_login=success and
      // load() picks up the new session on the next mount.
    } catch (e) {
      error.value = formatPlatformAuthError(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function loginLocal(input: {
    username: string
    password: string
    captchaId: string
    captcha: string
  }) {
    loading.value = true
    error.value = null
    try {
      session.value = await api.localLogin(input)
      error.value = null
      const settings = useSettingsStore()
      await settings.load()
      window.location.reload()
    } catch (e) {
      error.value = formatPlatformAuthError(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function cancelLogin() {
    await api.cancelPlatformLogin()
  }

  async function logout() {
    await api.logoutPlatform()
    session.value = { logged_in: false }
    lastFreshAtMs = 0
  }

  /** Mark quota exhausted in UI after a live balance gate failure (run_chat). */
  function markTokenQuotaExhausted() {
    if (!session.value.logged_in) return
    session.value = { ...session.value, tokenQuotaExhausted: true }
  }

  return {
    session,
    loading,
    error,
    authMode,
    isStandalone,
    isPlatformAdmin,
    tokenQuotaExhausted,
    load,
    loadAuthMode,
    ensureFreshSession,
    requireSession,
    loginHint,
    formatLoginGateError,
    login,
    loginLocal,
    cancelLogin,
    logout,
    markTokenQuotaExhausted
  }
})
