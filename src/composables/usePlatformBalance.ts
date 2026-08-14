import { computed, onScopeDispose, ref, watch } from 'vue'
import { getCloudPlatformMe } from '../lib/cloudAgents'
import { isTauriRuntime } from '../lib/runtime'
import { usePlatformAuthStore } from '../stores/platformAuth'

export const LOW_PLATFORM_BALANCE_YUAN = 10

export function isLowPlatformBalance(balance: string | null | undefined): boolean {
  if (balance == null || balance.trim() === '') return false
  const value = Number(balance)
  return Number.isFinite(value) && value >= 0 && value < LOW_PLATFORM_BALANCE_YUAN
}

export function shouldShowPlatformBalance(
  isDesktopRuntime: boolean,
  isStandalone: boolean,
  loggedIn: boolean
): boolean {
  return isDesktopRuntime && !isStandalone && loggedIn
}

/**
 * Desktop-only account balance for small persistent UI surfaces.
 * Fetch failures deliberately stay silent so the rest of the app remains usable.
 */
export function usePlatformBalance() {
  const platformAuth = usePlatformAuthStore()
  const balance = ref<string | null>(null)
  const loading = ref(false)
  let requestId = 0

  const visible = computed(() =>
    shouldShowPlatformBalance(
      isTauriRuntime(),
      platformAuth.isStandalone,
      platformAuth.session.logged_in
    )
  )
  const lowBalance = computed(() => isLowPlatformBalance(balance.value))
  const exhausted = computed(() => visible.value && platformAuth.tokenQuotaExhausted)

  async function refresh() {
    if (!visible.value || loading.value) return
    const currentRequest = ++requestId
    loading.value = true
    try {
      const me = await getCloudPlatformMe()
      if (currentRequest === requestId) balance.value = me.balance_yuan
    } catch {
      if (currentRequest === requestId) balance.value = null
    } finally {
      if (currentRequest === requestId) loading.value = false
    }
  }

  const stop = watch(
    [visible, () => platformAuth.tokenQuotaExhausted],
    ([isVisible]) => {
      if (!isVisible) {
        balance.value = null
        return
      }
      void refresh()
    },
    { immediate: true }
  )
  onScopeDispose(stop)

  return {
    balance,
    loading,
    visible,
    lowBalance,
    exhausted,
    refresh
  }
}
