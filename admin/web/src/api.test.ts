import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, api, errText } from './api'

/** 用一个受控的 fetch 替身，断言请求形状与错误映射。 */
function mockFetch(handler: (url: string, init: RequestInit) => Response | Promise<Response>) {
  const spy = vi.fn(async (url: any, init: any) => handler(String(url), init))
  vi.stubGlobal('fetch', spy)
  return spy
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

describe('api 请求构造', () => {
  beforeEach(() => vi.unstubAllGlobals())
  afterEach(() => vi.unstubAllGlobals())

  it('GET 不带 body，也不带 content-type', async () => {
    const spy = mockFetch(() => json({ ok: true }))
    await api.get('/api/admin/me')
    const init = spy.mock.calls[0]![1] as RequestInit
    expect(init.method).toBe('GET')
    expect(init.body).toBeUndefined()
    expect(init.headers).toBeUndefined()
  })

  it('POST 带 JSON body 和 content-type', async () => {
    const spy = mockFetch(() => json({ id: 'x' }))
    await api.post('/api/admin/users', { username: 'a' })
    const init = spy.mock.calls[0]![1] as RequestInit
    expect(init.method).toBe('POST')
    expect(init.body).toBe('{"username":"a"}')
    expect(init.headers).toEqual({ 'content-type': 'application/json' })
  })

  it('始终带 cookie（会话是 HttpOnly cookie）', async () => {
    const spy = mockFetch(() => json({}))
    await api.get('/x')
    expect((spy.mock.calls[0]![1] as RequestInit).credentials).toBe('include')
  })

  it('返回解析后的 JSON', async () => {
    mockFetch(() => json({ user: { id: '1' } }))
    await expect(api.get('/api/admin/me')).resolves.toEqual({ user: { id: '1' } })
  })

  it('204 空响应解析成 null，不抛错', async () => {
    mockFetch(() => new Response(null, { status: 204 }))
    await expect(api.del('/x')).resolves.toBeNull()
  })
})

describe('api 错误映射', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('错误响应抛出 ApiError，带 status / message / code', async () => {
    mockFetch(() => json({ error: 'bad_request', message: '资源已存在' }, 400))
    const err = await api.post('/x', {}).catch(e => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(400)
    expect(err.message).toBe('资源已存在')
    expect(err.code).toBe('bad_request')
  })

  it('非 JSON 错误体退化成原文', async () => {
    mockFetch(() => new Response('Bad Gateway', { status: 502 }))
    const err = await api.get('/x').catch(e => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(502)
    expect(err.message).toBe('Bad Gateway')
  })

  it('没有 message 字段时给兜底文案', async () => {
    mockFetch(() => json({}, 500))
    const err = await api.get('/x').catch(e => e)
    expect(err.message).toContain('HTTP 500')
  })

  it('401 也走同一条错误路径（交给上层决定跳登录）', async () => {
    mockFetch(() => json({ error: 'unauthorized', message: '未登录' }, 401))
    const err = await api.get('/api/admin/me').catch(e => e)
    expect(err.status).toBe(401)
  })
})

describe('errText', () => {
  it('ApiError 取 message', () => {
    expect(errText(new ApiError(400, '出错了', 'bad_request'))).toBe('出错了')
  })

  it('普通 Error 取 message', () => {
    expect(errText(new Error('boom'))).toBe('boom')
  })

  it('非 Error 值转字符串', () => {
    expect(errText('字符串错误')).toBe('字符串错误')
  })
})
