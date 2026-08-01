/** User-facing login gate copy (Composer / send / cloud). */
export type LoginRequiredPurpose = 'default' | 'attachment' | 'cloud'

export function loginRequiredMessage(
  isStandalone: boolean,
  purpose: LoginRequiredPurpose = 'default'
): string {
  if (purpose === 'cloud') return '请先登录 Pointer 平台账户'
  if (isStandalone) {
    return purpose === 'attachment' ? '请先登录后再添加附件' : '请先登录'
  }
  return purpose === 'attachment'
    ? '请先登录 Pointer 账户后再添加附件'
    : '请先登录 Pointer 账户'
}

/** Map backend/login-gate errors to the standard login hint; otherwise null. */
export function mapLoginGateError(
  message: string,
  isStandalone: boolean,
  purpose: LoginRequiredPurpose = 'default'
): string | null {
  if (
    message.includes('platform_login_required') ||
    message.includes('local_login_required') ||
    message.includes('请先登录')
  ) {
    return loginRequiredMessage(isStandalone, purpose)
  }
  return null
}
