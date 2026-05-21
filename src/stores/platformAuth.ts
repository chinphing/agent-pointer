import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../lib/api'

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
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
      session.value = await api.getPlatformSession()
      if (session.value.logged_in) {
        session.value = await api.refreshPlatformSession()
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
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
      error.value = e instanceof Error ? e.message : String(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function logout() {
    await api.logoutPlatform()
    session.value = { logged_in: false }
  }

  return { session, loading, error, load, login, logout }
})
