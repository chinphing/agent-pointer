import { describe, expect, it } from 'vitest'
import { buildCompressionProgressLabel } from './compressionMessage'

describe('buildCompressionProgressLabel', () => {
  it('uses main-thread copy by default', () => {
    expect(buildCompressionProgressLabel({ scope: 'main' })).toBe('正在压缩较早记录')
  })

  it('includes sub-agent name', () => {
    expect(
      buildCompressionProgressLabel({ scope: 'sub_agent', subAgentName: 'explore' })
    ).toBe('explore 子任务内正在压缩较早记录')
  })
})
