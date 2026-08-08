import { describe, expect, it } from 'vitest'
import { dataUrlToBytes, dataUrlToContentBase64 } from './saveLocalFile'

describe('dataUrlToBytes / dataUrlToContentBase64', () => {
  it('decodes base64 PNG-style data URLs', () => {
    const text = 'hello'
    const b64 = btoa(text)
    const bytes = dataUrlToBytes(`data:image/png;base64,${b64}`)
    expect(new TextDecoder().decode(bytes)).toBe(text)
    expect(dataUrlToContentBase64(`data:image/png;base64,${b64}`)).toBe(b64)
  })

  it('decodes URI-encoded SVG data URLs used by markdown export', () => {
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><text>架构</text></svg>'
    const dataUrl = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg)
    const bytes = dataUrlToBytes(dataUrl)
    expect(new TextDecoder().decode(bytes)).toBe(svg)
    // Round-trip through base64 must still yield the original UTF-8 SVG.
    const asB64 = dataUrlToContentBase64(dataUrl)
    const roundTrip = Uint8Array.from(atob(asB64), c => c.charCodeAt(0))
    expect(new TextDecoder().decode(roundTrip)).toBe(svg)
  })

  it('rejects treating URI payload as base64 (regression for desktop SVG save)', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"></svg>'
    const uriPayload = encodeURIComponent(svg)
    // Old bug: slice after comma and feed to base64 decoder → throws / garbage.
    expect(() => atob(uriPayload)).toThrow()
    expect(new TextDecoder().decode(dataUrlToBytes(`data:image/svg+xml;charset=utf-8,${uriPayload}`))).toBe(
      svg
    )
  })
})
