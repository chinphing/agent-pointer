import { reactive } from 'vue'
import { api } from './api'

export interface SessionUser {
  id: string
  username: string
  displayName: string
  status?: string
  phone?: string | null
  lastLoginAt?: number | null
}

export const session = reactive({
  ready: false,
  user: null as SessionUser | null,
  roles: [] as string[],
  permissions: [] as string[],
})

export function can(permission: string): boolean {
  return session.permissions.includes(permission)
}

/** 权限前缀判断：`canAny('provider')` 命中 provider:read/write/delete。 */
export function canAny(domain: string): boolean {
  return session.permissions.some((p) => p.startsWith(`${domain}:`))
}

export async function loadSession(): Promise<void> {
  try {
    const me = await api.get('/api/admin/me')
    session.user = me.user
    session.roles = me.roles || []
    session.permissions = me.permissions || []
  } catch {
    session.user = null
    session.roles = []
    session.permissions = []
  } finally {
    session.ready = true
  }
}

export async function login(
  username: string,
  password: string,
  captchaId: string,
  captcha: string
): Promise<void> {
  const r = await api.post('/api/admin/auth/login', { username, password, captchaId, captcha })
  session.user = r.user
  session.roles = r.roles || []
  session.permissions = r.permissions || []
  session.ready = true
  // 登录响应只带最小字段；再拉一次 `/me` 补上 status / phone / lastLoginAt，
  // 否则「我的账户」里状态会显示成「停用」、手机号显示成「未设置」。
  await loadSession()
}

/**
 * 短信登录。后端在响应里种会话 Cookie，所以这里只要把会话读回来。
 *
 * 只有私有部署会注册这个接口（开源版没有短信能力），调用方要先探测
 * `/api/auth/sms/enabled` 再决定是否展示入口。
 */
export async function loginWithSms(phone: string, code: string): Promise<void> {
  await api.post('/api/auth/sms/login', { phone, code })
  await loadSession()
  if (!session.user) throw new Error('登录失败')
}

export async function logout(): Promise<void> {
  try {
    await api.post('/api/admin/auth/logout')
  } catch {
    // 本地状态照清：接口失败（网络/会话已失效）不该让用户卡在「已登录」界面，
    // 也不该把「登出失败」这种假错误弹给用户——登出意图已经达成了。
  } finally {
    session.user = null
    session.roles = []
    session.permissions = []
  }
}
