import { beforeEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
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

const { useIsAppAdmin, shouldShowAdminDebugSection } = await import('./useAppAdmin')

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

/**
 * 回归：设置页的调试分区曾经把 `platformAuth.isStandalone` 也算作管理员信号。
 *
 * standalone 部署下这一项对所有浏览器会话都成立，于是接入这台机器的 SSO 用户
 * 也看到了调试分区。现在与后端 `effective_settings_view().is_platform_admin`
 * 同一口径：只看会话票标记与后端管理员判定，standalone 本身不再放行。
 */
describe('shouldShowAdminDebugSection', () => {
  it('hides the debug section for an SSO user on a standalone install', () => {
    expect(shouldShowAdminDebugSection({ settingsAdmin: false, sessionAdmin: false })).toBe(
      false
    )
  })

  it('shows it for the desktop local operator, which the backend reports as admin', () => {
    expect(shouldShowAdminDebugSection({ settingsAdmin: true, sessionAdmin: false })).toBe(
      true
    )
  })

  it('shows it for a platform admin on a bound deployment', () => {
    expect(shouldShowAdminDebugSection({ settingsAdmin: false, sessionAdmin: true })).toBe(
      true
    )
  })

  it('is what SettingsDialog actually uses, without isStandalone', () => {
    const sfc = readFileSync(
      new URL('../components/settings/SettingsDialog.vue', import.meta.url),
      'utf8'
    )
    const start = sfc.indexOf('const showAdminDebugSection')
    expect(start).toBeGreaterThan(-1)
    const gate = sfc.slice(start, sfc.indexOf('interface SidebarItem', start))
    expect(gate).toContain('shouldShowAdminDebugSection({')
    expect(gate).not.toContain('isStandalone')
  })
})
