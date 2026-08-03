import { describe, expect, it } from 'vitest'
import {
  UPLOAD_ABORTED_MESSAGE,
  isUploadAbortedError,
  mapUploadTransferPercent
} from './multipartUpload'

describe('mapUploadTransferPercent', () => {
  it('maps mid-transfer bytes into 0–100', () => {
    expect(mapUploadTransferPercent(50, 100)).toBe(50)
    expect(mapUploadTransferPercent(0, 100)).toBe(0)
  })

  it('reaches 100% when all bytes are sent', () => {
    expect(mapUploadTransferPercent(100, 100)).toBe(100)
    expect(mapUploadTransferPercent(999, 1000)).toBe(100)
  })

  it('returns 0 for invalid totals', () => {
    expect(mapUploadTransferPercent(10, 0)).toBe(0)
    expect(mapUploadTransferPercent(10, -1)).toBe(0)
  })
})

describe('isUploadAbortedError', () => {
  it('detects cancelled uploads', () => {
    expect(isUploadAbortedError(new Error(UPLOAD_ABORTED_MESSAGE))).toBe(true)
    expect(isUploadAbortedError(new Error('网络错误'))).toBe(false)
  })
})
