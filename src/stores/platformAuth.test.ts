import { describe, expect, it } from 'vitest'
import {
  extractPlatformAuthHttpStatus,
  isPlatformAuthTransientError
} from './platformAuth'

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
    expect(isPlatformAuthTransientError('网络异常，暂时无法验证登录态，请稍后重试')).toBe(true)
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
