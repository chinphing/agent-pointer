import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../lib/api'

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
}

function formatPlatformAuthError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('oauth callback timeout')) return '登录超时，请重试'
  if (msg.includes('platform_login_cancelled')) return '已取消登录'
  return msg
}

export const usePlatformAuthStore = defineStore('platformAuth', () => {
  const session = ref<PlatformSessionView>({ logged_in: false })
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function load() {
    loading.value = true
    error.value = null
    try {
      await api.loadPlatformSessionFromKeyring()
      try {
        session.value = await api.refreshPlatformSession()
      } catch {
        session.value = await api.getPlatformSession()
      }
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

  return { session, loading, error, load, login, cancelLogin, logout }
})
