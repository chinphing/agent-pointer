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

function formatPlatformAuthError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('oauth callback timeout')) return '登录超时，请重试'
  if (msg.includes('platform_login_cancelled')) return '已取消登录'
  if (msg.includes('本机回环')) {
    return '本机回环不可用：请检查防火墙、安全软件、VPN 或系统代理是否拦截 localhost，然后重试'
  }
  if (msg.includes('invalid_refresh_token')) return '登录已失效，请重新登录 Pointer 账户'
  if (msg.includes('server_access_denied')) return '此 Server 未授权您的账户，请联系管理员'
  if (msg.includes('invalid_captcha')) return '验证码错误，请重试'
  if (msg.includes('invalid_credentials')) return '账号或密码错误'
  if (msg.includes('local_login_required')) return '请先登录'
  if (msg.includes('Plugin not found') || msg.includes('not allowed')) {
    return '当前为云主机页面，登录态由平台自动注入，无需再次登录'
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
      error.value = formatPlatformAuthError(e)
      session.value = { logged_in: false }
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
      error.value = formatPlatformAuthError(e)
      if ((e instanceof Error ? e.message : String(e)).includes('invalid_refresh_token')) {
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
