import { describe, expect, it } from 'vitest'
import { stripWireAttachmentFields, userMessageDisplayContent } from './messageNormalizer'
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

describe('userMessageDisplayContent', () => {
  it('prefers uiBindings.bubbleText over full content', () => {
    expect(
      userMessageDisplayContent({
        id: 'p0',
        role: 'user',
        content: '后台任务已完成。\n\nfull body for the model\nalready claimed',
        status: 'done',
        createdAt: 0,
        uiBindings: {
          hostKind: 'idle_job_push',
          bubbleText: '后台任务已完成。',
        },
      })
    ).toBe('后台任务已完成。')
  })

  it('prefers a specific idle-push bubbleText over headings in content', () => {
    expect(
      userMessageDisplayContent({
        id: 'p0b',
        role: 'user',
        content: '后台任务已完成。\n\n### 搜索登录\nkind: subagent\nstatus: completed\njobId: job_a\n',
        status: 'done',
        createdAt: 0,
        uiBindings: {
          hostKind: 'idle_job_push',
          bubbleText: '后台任务已完成：铺线 1',
        },
      })
    ).toBe('后台任务已完成：铺线 1')
  })

  it('enriches a generic idle-push bubble from job headings in content', () => {
    expect(
      userMessageDisplayContent({
        id: 'p0c',
        role: 'user',
        content:
          '后台任务已完成。\n\n### 线 1（约堡都会区）\nkind: subagent\nstatus: completed\njobId: job_a\n\n### 线 2\nkind: subagent\nstatus: failed\njobId: job_b\n',
        status: 'done',
        createdAt: 0,
        uiBindings: {
          hostKind: 'idle_job_push',
          bubbleText: '后台任务已完成。',
        },
      })
    ).toBe('后台任务已完成：线 1（约堡都会区）、线 2')
  })

  it('summarizes more than two idle-push headings and uses failure prefix', () => {
    expect(
      userMessageDisplayContent({
        id: 'p0d',
        role: 'user',
        content:
          '后台任务已完成。\n\n### 线 1\nkind: subagent\nstatus: failed\njobId: a\n\n### 线 2\nkind: subagent\nstatus: failed\njobId: b\n\n### 线 3\nkind: subagent\nstatus: failed\njobId: c\n',
        status: 'done',
        createdAt: 0,
        uiBindings: { hostKind: 'idle_job_push' },
      })
    ).toBe('后台任务失败：线 1、线 2 等 3 个')
  })

  it('falls back to short line when only hostKind is set', () => {
    expect(
      userMessageDisplayContent({
        id: 'p1',
        role: 'user',
        content: '后台任务已完成。\n\nThese finished background results are in this turn.\n',
        status: 'done',
        createdAt: 0,
        uiBindings: { hostKind: 'idle_job_push' },
      })
    ).toBe('后台任务已完成。')
  })

  it('shows full content when uiBindings is absent', () => {
    expect(
      userMessageDisplayContent({
        id: 'p2',
        role: 'user',
        content: '后台任务已完成。\n\nalready claimed\nbody',
        status: 'done',
        createdAt: 0,
      })
    ).toBe('后台任务已完成。\n\nalready claimed\nbody')
  })
})
