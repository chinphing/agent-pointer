import { describe, expect, it } from 'vitest'
import { stripWireAttachmentFields } from './messageNormalizer'
import type { ChatMessage, MediaAttachment } from '../types/chat'

function att(over: Partial<MediaAttachment>): MediaAttachment {
  return {
    id: 'a1',
    kind: 'image',
    mimeType: 'image/png',
    fileName: 'x.png',
    sizeBytes: 10,
    ...over
  }
}

function msg(attachments: MediaAttachment[]): ChatMessage {
  return { id: 'm1', role: 'user', content: 'hi', status: 'done', createdAt: 0, attachments }
}

describe('stripWireAttachmentFields', () => {
  it('strips data: previewUrl before disk persist (UI-only base64)', () => {
    const [out] = stripWireAttachmentFields([
      msg([att({ previewUrl: 'data:image/png;base64,QUJD' })])
    ])
    expect(out.attachments?.[0].previewUrl).toBeUndefined()
  })

  it('keeps https previewUrl (OSS remote)', () => {
    const [out] = stripWireAttachmentFields([
      msg([att({ previewUrl: 'https://cdn.example.com/x.png' })])
    ])
    expect(out.attachments?.[0].previewUrl).toBe('https://cdn.example.com/x.png')
  })

  it('keeps audio contentBase64 when no storageRelPath', () => {
    const [out] = stripWireAttachmentFields([
      msg([att({ kind: 'audio', mimeType: 'audio/mpeg', contentBase64: 'QUJD' })])
    ])
    expect(out.attachments?.[0].contentBase64).toBe('QUJD')
  })

  it('strips contentBase64 when storageRelPath is present', () => {
    const [out] = stripWireAttachmentFields([
      msg([att({ contentBase64: 'QUJD', storageRelPath: 'conv/a1.mp3' })])
    ])
    expect(out.attachments?.[0].contentBase64).toBeUndefined()
    expect(out.attachments?.[0].storageRelPath).toBe('conv/a1.mp3')
  })

  it('strips legacy data: previewUrl reloaded from disk on hydration', () => {
    const [out] = stripWireAttachmentFields([
      msg([
        att({
          previewUrl: 'data:image/png;base64,QUJD',
          storageRelPath: 'conv/a1.png'
        })
      ])
    ])
    expect(out.attachments?.[0].previewUrl).toBeUndefined()
    expect(out.attachments?.[0].storageRelPath).toBe('conv/a1.png')
  })
})
