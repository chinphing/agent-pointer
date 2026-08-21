import { describe, expect, it } from 'vitest'
import { buildCompressionNoticeContent, buildCompressionProgressLabel } from './compressionMessage'

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

describe('buildCompressionNoticeContent', () => {
  it('does not mention keep-N user turns', () => {
    expect(
      buildCompressionNoticeContent({
        reason: 'budget',
        messagesBefore: 10,
        messagesAfter: 4,
        droppedCount: 6,
        keepRecentUserTurns: 6,
        scope: 'main'
      })
    ).toBe('【压缩】已将较早 6 条对话摘要为 1 条，并保留最近对话原文。')
  })

  it('uses in-run copy for current-turn compression', () => {
    expect(
      buildCompressionNoticeContent({
        reason: 'in_run',
        messagesBefore: 40,
        messagesAfter: 12,
        droppedCount: 28,
        keepRecentUserTurns: 1,
        scope: 'main'
      })
    ).toBe('【压缩】已将当前轮次 28 条过程摘要为 1 条，并保留你的消息与最近原文。')
  })
})
