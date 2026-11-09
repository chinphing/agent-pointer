import { t } from '../i18n'

/**
 * Chat needs a sign-in before settings and API keys are available.
 * Desktop standalone has no account. Browser standalone still uses the local account.
 */
export function needsSignInBeforeChat(input: {
  loggedIn: boolean
  isStandalone: boolean
  isTauri: boolean
}): boolean {
  return !input.loggedIn && !(input.isStandalone && input.isTauri)
}

/** User-facing login gate copy (Composer / send). */
export type LoginRequiredPurpose = 'default' | 'attachment'

export function loginRequiredMessage(
  isStandalone: boolean,
  purpose: LoginRequiredPurpose = 'default'
): string {
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
