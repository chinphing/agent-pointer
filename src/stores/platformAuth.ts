import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import * as api from '../lib/api'
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
  if (msg.includes('invalid_refresh_token')) return '登录已失效，请重新登录 Pointer 账户'
  return msg
}

/** Desktop: actively refresh/restore from auth.dat. Web: stub session. */
async function resolvePlatformSession(): Promise<PlatformSessionView> {
  if (isTauriRuntime()) {
    return api.refreshPlatformSession()
  }
  return api.getPlatformSession()
}

export const usePlatformAuthStore = defineStore('platformAuth', () => {
  const session = ref<PlatformSessionView>({ logged_in: false })
  const loading = ref(false)
  const error = ref<string | null>(null)

  const isPlatformAdmin = computed(() => session.value.isPlatformAdmin === true)
  const tokenQuotaExhausted = computed(
    () => session.value.logged_in && session.value.tokenQuotaExhausted === true
  )

  async function load() {
    loading.value = true
    error.value = null
    try {
      session.value = await resolvePlatformSession()
      const settings = useSettingsStore()
      await settings.load()
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
          const settings = useSettingsStore()
          await settings.load()
        }
      } else {
        session.value = await api.getPlatformSession()
      }
      return session.value
    } catch (e) {
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
      await api.openPlatformLogin()
      session.value = await api.refreshPlatformSession()
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

  return {
    session,
    loading,
    error,
    isPlatformAdmin,
    tokenQuotaExhausted,
    load,
    ensureFreshSession,
    login,
    cancelLogin,
    logout
  }
})
