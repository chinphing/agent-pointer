import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import * as api from '../lib/api'
import type { AuthMode } from '../lib/api'
import { isTauriRuntime } from '../lib/runtime'
import { useSettingsStore } from './settings'

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
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
  if (msg.includes('登录已失效')) return false
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

function formatPlatformAuthError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('oauth callback timeout')) return '登录超时，请重试'
  if (msg.includes('platform_login_cancelled')) return '已取消登录'
  if (msg.includes('本机回环')) {
    return '本机回环不可用：请检查防火墙、安全软件、VPN 或系统代理是否拦截 localhost，然后重试'
  }
  const status = extractPlatformAuthHttpStatus(msg)
  if (
    msg.includes('invalid_refresh_token') ||
    msg.includes('platform_token_expired') ||
    (status != null && isAuthFailureHttpStatus(status))
  ) {
    return '登录已失效，请重新登录 Pointer 账户'
  }
  if (msg.includes('server_access_denied')) return '此 Server 未授权您的账户，请联系管理员'
  if (msg.includes('invalid_captcha')) return '验证码错误，请重试'
  if (msg.includes('invalid_credentials')) return '账号或密码错误'
  if (msg.includes('local_login_required')) return '请先登录'
  if (msg.includes('Plugin not found') || msg.includes('not allowed')) {
    return '当前为云主机页面，登录态由平台自动注入，无需再次登录'
  }
  if (isPlatformAuthTransientError(msg)) {
    return '网络异常，暂时无法验证登录态，请稍后重试'
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
    return '此 Server 未授权您的账户，请联系管理员'
  }
  if (ssoError) {
    if (ssoError === 'not_configured') return 'SSO 未配置，请联系管理员'
    if (ssoError === 'not_standalone') return '当前实例不支持 SSO 登录'
    if (ssoError === 'missing') return '缺少 SSO 票据'
    return 'SSO 登录失败，请重新从门户打开'
  }
  return errorMsg ? decodeURIComponent(errorMsg) : null
}

export const usePlatformAuthStore = defineStore('platformAuth', () => {
  const session = ref<PlatformSessionView>({ logged_in: false })
  const loading = ref(false)
  const error = ref<string | null>(null)
  const authMode = ref<AuthMode>('platform')

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

  async function ensureFreshSession(): Promise<PlatformSessionView> {
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
      }
      throw new Error(error.value || '平台登录态刷新失败')
    }
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
    login,
    loginLocal,
    cancelLogin,
    logout,
    markTokenQuotaExhausted
  }
})
