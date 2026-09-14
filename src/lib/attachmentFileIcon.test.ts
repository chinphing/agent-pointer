import { describe, expect, it } from 'vitest'
import { attachmentFileBadge, attachmentFileExtension } from './attachmentFileIcon'

describe('attachmentFileBadge', () => {
  it('reads extension from basename', () => {
    expect(attachmentFileExtension('Tanzania_Audio_Leads_Final_256.xlsx')).toBe('xlsx')
    expect(attachmentFileExtension('/tmp/a.PDF')).toBe('pdf')
    expect(attachmentFileExtension('noext')).toBe('')
  })

  it('maps common document types to familiar badges', () => {
    expect(attachmentFileBadge('a.xlsx')).toEqual({ label: 'XLS', tone: 'sheet' })
    expect(attachmentFileBadge('a.csv')).toEqual({ label: 'CSV', tone: 'sheet' })
    expect(attachmentFileBadge('a.docx')).toEqual({ label: 'DOC', tone: 'word' })
    expect(attachmentFileBadge('a.pptx')).toEqual({ label: 'PPT', tone: 'slides' })
    expect(attachmentFileBadge('a.pdf')).toEqual({ label: 'PDF', tone: 'pdf' })
    expect(attachmentFileBadge('a.zip')).toEqual({ label: 'ZIP', tone: 'zip' })
    expect(attachmentFileBadge('a.json')).toEqual({ label: 'JSON', tone: 'code' })
    expect(attachmentFileBadge('a.ts')).toEqual({ label: 'TS', tone: 'code' })
    expect(attachmentFileBadge('a.md')).toEqual({ label: 'MD', tone: 'text' })
  })

  it('falls back via mime when extension missing', () => {
    expect(attachmentFileBadge('export', 'application/pdf')).toEqual({
      label: 'PDF',
      tone: 'pdf'
    })
    expect(
      attachmentFileBadge(
        'export',
        'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'
      )
    ).toEqual({ label: 'XLS', tone: 'sheet' })
    expect(attachmentFileBadge('mystery')).toEqual({ label: 'FILE', tone: 'file' })
  })
})
