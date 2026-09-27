import { describe, expect, it } from 'vitest'
import { t } from '../i18n'
import {
  buildCompressionNoticeContent,
  buildCompressionProgressLabel,
  isInRunCompressionSummaryMessage,
  isParentThreadCompressionProgress,
  isPrefixCompressionSummaryMessage
} from './compressionMessage'

describe('compression summary roles', () => {
  it('treats user summaries as prefix and assistant summaries as in-run', () => {
    const prefix = {
      id: 'p',
      role: 'user' as const,
      content: '[Conversation summary (auto-compression)]\nbody',
      status: 'done' as const,
      createdAt: 1
    }
    const inRun = {
      id: 'r',
      role: 'assistant' as const,
      content: '[Conversation summary (auto-compression)]\nmid',
      status: 'done' as const,
      createdAt: 2,
      toolCalls: []
    }
    expect(isPrefixCompressionSummaryMessage(prefix)).toBe(true)
    expect(isInRunCompressionSummaryMessage(prefix)).toBe(false)
    expect(isInRunCompressionSummaryMessage(inRun)).toBe(true)
    expect(isPrefixCompressionSummaryMessage(inRun)).toBe(false)
  })
})

describe('buildCompressionProgressLabel', () => {
  it('uses main-thread copy by default', () => {
    expect(buildCompressionProgressLabel({ scope: 'main' })).toBe(t('chat.compression.progressMain'))
  })

  it('includes sub-agent name', () => {
    expect(
      buildCompressionProgressLabel({ scope: 'sub_agent', subAgentName: 'explore' })
    ).toBe(t('chat.compression.progressSubAgent', { name: 'explore' }))
  })

  it('uses parent-thread copy inside the sub-agent frame', () => {
    expect(
      buildCompressionProgressLabel({
        scope: 'sub_agent',
        subAgentName: 'explore',
        inSubAgentFrame: true
      })
    ).toBe(t('chat.compression.progressMain'))
  })
})

describe('isParentThreadCompressionProgress', () => {
  it('keeps the lead-thread marker on the parent list', () => {
    expect(isParentThreadCompressionProgress({ scope: 'main' })).toBe(true)
  })

  it('does not park a sub-agent cut on the parent process list', () => {
    expect(isParentThreadCompressionProgress({ scope: 'sub_agent' })).toBe(false)
  })

  it('rejects a missing progress state', () => {
    expect(isParentThreadCompressionProgress(null)).toBe(false)
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
    ).toBe(t('chat.compression.budgetNotice', { dropped: 6 }))
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
    ).toBe(t('chat.compression.inRunNotice', { dropped: 28 }))
  })
})
