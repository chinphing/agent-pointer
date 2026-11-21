import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { workspaceFileMediaObjectUrl } from './web'

/**
 * Sandbox tiers resolve the workspace against the conversation's environment,
 * so the media preview request must carry `conversationId` (same as
 * `/api/workspace/file`). Without it the server fails closed.
 */
describe('workspaceFileMediaObjectUrl', () => {
  let fetchMock: ReturnType<typeof vi.fn>
  let createObjectURL: ReturnType<typeof vi.fn>
  const originalCreateObjectURL = URL.createObjectURL

  beforeEach(() => {
    // `web.ts` uses `window.setTimeout` for its abort timer.
    vi.stubGlobal('window', globalThis)
    fetchMock = vi.fn().mockResolvedValue(new Response(new Blob(['png']), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)
    createObjectURL = vi.fn(() => 'blob:mock-url')
    ;(URL as unknown as { createObjectURL: unknown }).createObjectURL = createObjectURL
  })

  afterEach(() => {
    ;(URL as unknown as { createObjectURL: unknown }).createObjectURL = originalCreateObjectURL
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  function requestedUrl(): URL {
    const [raw] = fetchMock.mock.calls[0] as [string]
    return new URL(raw, 'http://localhost')
  }

  it('carries conversationId on the file-media request', async () => {
    const url = await workspaceFileMediaObjectUrl('/ws/root', 'shots/a.png', 'conv-123')

    expect(url).toBe('blob:mock-url')
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const requestUrl = requestedUrl()
    expect(requestUrl.pathname).toBe('/api/workspace/file-media')
    expect(requestUrl.searchParams.get('workspaceRoot')).toBe('/ws/root')
    expect(requestUrl.searchParams.get('relativePath')).toBe('shots/a.png')
    expect(requestUrl.searchParams.get('conversationId')).toBe('conv-123')
  })

  it('omits conversationId when the caller has none (local tier stays compatible)', async () => {
    await workspaceFileMediaObjectUrl('/ws/root', 'shots/a.png')

    const requestUrl = requestedUrl()
    expect(requestUrl.pathname).toBe('/api/workspace/file-media')
    expect(requestUrl.searchParams.has('conversationId')).toBe(false)
  })

  it('omits an empty conversationId instead of sending a blank param', async () => {
    await workspaceFileMediaObjectUrl('/ws/root', 'shots/a.png', '')

    expect(requestedUrl().searchParams.has('conversationId')).toBe(false)
  })
})
