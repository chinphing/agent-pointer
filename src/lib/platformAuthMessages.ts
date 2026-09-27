import { t } from '../i18n'

/** User-facing login gate copy (Composer / send / cloud). */
export type LoginRequiredPurpose = 'default' | 'attachment' | 'cloud'

export function loginRequiredMessage(
  isStandalone: boolean,
  purpose: LoginRequiredPurpose = 'default'
): string {
  if (purpose === 'cloud') return t('auth.loginRequiredPointer')
  if (isStandalone) {
    return purpose === 'attachment'
      ? t('auth.loginRequiredAttach')
      : t('auth.loginRequired')
  }
  return purpose === 'attachment'
    ? t('auth.loginRequiredPointerAttach')
    : t('auth.loginRequiredPointer')
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
    message.includes('请先登录') ||
    message.includes('Sign in')
  ) {
    return loginRequiredMessage(isStandalone, purpose)
  }
  return null
}
