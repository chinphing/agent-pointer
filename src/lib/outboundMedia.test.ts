import { describe, expect, it } from 'vitest'
import {
  extractOutboundMediaPaths,
  splitAttachmentIdQuery,
  stripOutboundMediaMarkers
} from './outboundMedia'

describe('outboundMedia attachmentId', () => {
  it('splits attachmentId query', () => {
    expect(splitAttachmentIdQuery('/tmp/a.xlsx?attachmentId=abc123abc123')).toEqual({
      path: '/tmp/a.xlsx',
      attachmentId: 'abc123abc123'
    })
  })

  it('strips MEDIA lines including attachmentId for display', () => {
    const text = [
      'v2 线索池已导出：',
      '',
      'MEDIA:Tanzania_音响客户线索池_v2.xlsx?attachmentId=9f2f0b9a8b7c'
    ].join('\n')
    expect(stripOutboundMediaMarkers(text)).toBe('v2 线索池已导出：')
  })

  it('extracts path without query', () => {
    expect(
      extractOutboundMediaPaths('MEDIA:/tmp/a.xlsx?attachmentId=abc123abc123')
    ).toEqual(['/tmp/a.xlsx'])
  })
})
