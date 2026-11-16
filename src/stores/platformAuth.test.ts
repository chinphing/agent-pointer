import { beforeEach, describe, expect, it } from 'vitest'
import { t } from '../i18n'
import { applyUiLocale } from '../lib/uiLocale'
import { loginRequiredMessage, mapLoginGateError, needsSignInBeforeChat } from '../lib/platformAuthMessages'
import {
  extractPlatformAuthHttpStatus,
  isPlatformAuthTransientError,
  platformLoginErrorText
} from './platformAuth'

beforeEach(() => {
  applyUiLocale('zh-CN')
})

describe('platformAuth error classification', () => {
  it('extracts http_status tag first', () => {
    expect(
      extractPlatformAuthHttpStatus(
        'token exchange failed http_status=502 (502 Bad Gateway): oops'
      )
    ).toBe(502)
    expect(
      extractPlatformAuthHttpStatus('token exchange failed (401 Unauthorized): denied')
    ).toBe(401)
    expect(
      extractPlatformAuthHttpStatus('partner balance request failed: HTTP 503: unavailable')
    ).toBe(503)
    expect(extractPlatformAuthHttpStatus('token request failed: connection reset')).toBeNull()
  })

  it('treats 401/403 as auth failure, not transient', () => {
    expect(
      isPlatformAuthTransientError(
        'token exchange failed http_status=401 (401 Unauthorized): invalid_refresh_token'
      )
    ).toBe(false)
    expect(
      isPlatformAuthTransientError('token exchange failed http_status=403 (403 Forbidden): no')
    ).toBe(false)
  })

  it('treats 408/429/5xx and transport errors as transient', () => {
    expect(
      isPlatformAuthTransientError('token exchange failed http_status=502 (502 Bad Gateway): x')
    ).toBe(true)
    expect(
      isPlatformAuthTransientError('token exchange failed http_status=429 (429 Too Many Requests)')
    ).toBe(true)
    expect(isPlatformAuthTransientError('token request failed: error sending request')).toBe(true)
    expect(isPlatformAuthTransientError(t('auth.networkVerifyFailed'))).toBe(true)
  })

  it('does not treat bare "401" in unrelated text as an HTTP status', () => {
    expect(
      extractPlatformAuthHttpStatus('upstream gateway 401 while connecting to CDN')
    ).toBeNull()
    // Without http_status= tag, prose "401" must not force auth-failure classification.
    expect(
      isPlatformAuthTransientError('upstream gateway 401 while connecting to CDN')
    ).toBe(false)
    expect(
      isPlatformAuthTransientError(
        'token exchange failed http_status=401 (401 Unauthorized): revoked'
      )
    ).toBe(false)
  })

  it('does not retry other 4xx as transient network blips', () => {
    expect(
      isPlatformAuthTransientError('token exchange failed http_status=400 (400 Bad Request): bad')
    ).toBe(false)
  })
})

describe('loginRequiredMessage / mapLoginGateError', () => {
  it('returns purpose-specific copy for platform and standalone', () => {
    expect(loginRequiredMessage(false, 'default')).toBe(t('auth.loginRequiredPointer'))
    expect(loginRequiredMessage(true, 'default')).toBe(t('auth.loginRequired'))
    expect(loginRequiredMessage(false, 'attachment')).toBe(t('auth.loginRequiredPointerAttach'))
    expect(loginRequiredMessage(true, 'attachment')).toBe(t('auth.loginRequiredAttach'))
  })

  it('asks the browser standalone page to sign in, and leaves the desktop app alone', () => {
    expect(
      needsSignInBeforeChat({ loggedIn: false, isStandalone: true, isTauri: false })
    ).toBe(true)
    expect(
      needsSignInBeforeChat({ loggedIn: true, isStandalone: true, isTauri: false })
    ).toBe(false)
    expect(
      needsSignInBeforeChat({ loggedIn: false, isStandalone: true, isTauri: true })
    ).toBe(false)
    expect(
      needsSignInBeforeChat({ loggedIn: false, isStandalone: false, isTauri: false })
    ).toBe(true)
  })

  it('maps backend login-gate errors to the standard hint', () => {
    expect(mapLoginGateError('platform_login_required', false, 'attachment')).toBe(
      t('auth.loginRequiredPointerAttach')
    )
    expect(mapLoginGateError('local_login_required', true, 'default')).toBe(t('auth.loginRequired'))
    expect(mapLoginGateError('something else', false, 'default')).toBeNull()
  })
})

describe('platformLoginErrorText', () => {
  it('maps backend login error codes to localized copy', () => {
    expect(platformLoginErrorText('server_access_denied')).toBe(
      t('auth.error.server_access_denied')
    )
    expect(platformLoginErrorText('invalid_captcha')).toBe(t('auth.error.invalid_captcha'))
    expect(platformLoginErrorText('  provider_disabled  ')).toBe(
      t('auth.error.provider_disabled')
    )
  })

  it('finds a known code inside a wrapped error message', () => {
    expect(platformLoginErrorText('HTTP 429: rate_limited')).toBe(
      t('auth.error.rate_limited')
    )
    expect(platformLoginErrorText('请求失败 (429): login_capacity_reached')).toBe(
      t('auth.error.login_capacity_reached')
    )
  })

  it('never echoes an unknown code to the user', () => {
    const text = platformLoginErrorText('upstream_boom_500')
    expect(text).toBe(t('auth.loginFailed'))
    expect(text).not.toContain('upstream_boom_500')
  })

  it('returns empty text for an empty code', () => {
    expect(platformLoginErrorText('')).toBe('')
  })
})
