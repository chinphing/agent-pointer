import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import * as api from '../lib/api'
import { useSettingsStore } from './settings'

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
  isPlatformAdmin?: boolean
}

function formatPlatformAuthError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('oauth callback timeout')) return '登录超时，请重试'
  if (msg.includes('platform_login_cancelled')) return '已取消登录'
  return msg
}

/** Rust 启动时会 restore session 并注入 KEY；前端只轮询 session，避免重复 refresh/inject。 */
async function resolvePlatformSession(timeoutMs = 3000): Promise<PlatformSessionView> {
  const first = await api.getPlatformSession()
  if (first.logged_in) return first

  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 100))
    const next = await api.getPlatformSession()
    if (next.logged_in) return next
  }
  return first
}

export const usePlatformAuthStore = defineStore('platformAuth', () => {
  const session = ref<PlatformSessionView>({ logged_in: false })
  const loading = ref(false)
  const error = ref<string | null>(null)

  const isPlatformAdmin = computed(() => session.value.isPlatformAdmin === true)

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

  async function login() {
    loading.value = true
    error.value = null
    try {
      await api.openPlatformLogin()
      session.value = await api.getPlatformSession()
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

  return { session, loading, error, isPlatformAdmin, load, login, cancelLogin, logout }
})
