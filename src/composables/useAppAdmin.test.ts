import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const isTauriRuntime = vi.fn(() => false)
vi.mock('../lib/runtime', () => ({ isTauriRuntime: () => isTauriRuntime() }))

const platformAdmin = ref(false)
const settingsAdmin = ref(false)
vi.mock('../stores/platformAuth', () => ({
  usePlatformAuthStore: () => ({
    get isPlatformAdmin() {
      return platformAdmin.value
    }
  })
}))
vi.mock('../stores/settings', () => ({
  useSettingsStore: () => ({
    get isPlatformAdmin() {
      return settingsAdmin.value
    }
  })
}))

const { useIsAppAdmin } = await import('./useAppAdmin')

/**
 * 回归：桌面客户端不该被管理员门禁挡住。
 *
 * `c134b450` 为了服务端多租户把「设置 / 工作区面板」锁给平台管理员，但同一个
 * 判断也套在了桌面客户端上——本地登录的非管理员账号点「设置」毫无反应，连
 * 语言、通知、个性化都改不了。客户端里这些都是 `update_user_settings` 的
 * 用户级数据，没有平台可管，门禁只会把机主锁在自己应用外面。
 */
describe('useIsAppAdmin', () => {
  beforeEach(() => {
    isTauriRuntime.mockReturnValue(false)
    platformAdmin.value = false
    settingsAdmin.value = false
  })

  it('allows the desktop client without any admin role', () => {
    isTauriRuntime.mockReturnValue(true)
    expect(useIsAppAdmin().value).toBe(true)
  })

  it('keeps the gate on a platform deployment for non-admins', () => {
    expect(useIsAppAdmin().value).toBe(false)
  })

  it('allows a platform admin on a platform deployment', () => {
    platformAdmin.value = true
    expect(useIsAppAdmin().value).toBe(true)
  })

  it('allows a standalone admin', () => {
    settingsAdmin.value = true
    expect(useIsAppAdmin().value).toBe(true)
  })
})
