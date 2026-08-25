import { describe, expect, it } from 'vitest'
import {
  assistantDisplayKind,
  isDiscardableEmptyAssistant,
  isDiscardableEmptyAssistantOnCancel,
  isGenerationCancelledMessage
} from './assistantMessageKind'
import type { ChatMessage } from '../types/chat'

function msg(partial: Partial<ChatMessage>): ChatMessage {
  return {
    id: 'm1',
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 1,
    toolCalls: [],
    ...partial
  }
}

describe('isGenerationCancelledMessage', () => {
  it('matches Chinese stop and English cancelled variants', () => {
    expect(isGenerationCancelledMessage('已停止生成')).toBe(true)
    expect(isGenerationCancelledMessage('子 Agent 已停止')).toBe(true)
    expect(isGenerationCancelledMessage('cancelled')).toBe(true)
    expect(isGenerationCancelledMessage('Canceled')).toBe(true)
    expect(isGenerationCancelledMessage('request cancelled by user')).toBe(true)
    expect(isGenerationCancelledMessage('network failed')).toBe(false)
  })
})

describe('assistantDisplayKind cancelled', () => {
  it('renders empty cancel as muted cancelled kind', () => {
    expect(
      assistantDisplayKind(
        msg({ status: 'cancelled', errorMessage: '已停止生成', content: '' })
      )
    ).toBe('cancelled')
  })

  it('maps legacy error+cancelled text to cancelled kind when empty', () => {
    expect(
      assistantDisplayKind(msg({ status: 'error', errorMessage: 'cancelled', content: '' }))
    ).toBe('cancelled')
  })

  it('keeps model layout when cancel follows tools or text', () => {
    expect(
      assistantDisplayKind(
        msg({
          status: 'cancelled',
          content: '',
          toolCalls: [{ id: 't1', name: 'terminal', status: 'success', arguments: '{}' }]
        })
      )
    ).toBe('model')
    expect(
      assistantDisplayKind(msg({ status: 'cancelled', content: 'partial reply' }))
    ).toBe('model')
  })
})

describe('isDiscardableEmptyAssistantOnCancel', () => {
  it('discards streaming empty shells', () => {
    expect(isDiscardableEmptyAssistantOnCancel(msg({ status: 'streaming', content: '' }))).toBe(
      true
    )
  })

  it('keeps shells that already have tools', () => {
    expect(
      isDiscardableEmptyAssistantOnCancel(
        msg({
          status: 'streaming',
          toolCalls: [{ id: 't1', name: 'terminal', status: 'running', arguments: '{}' }]
        })
      )
    ).toBe(false)
  })
})

describe('isDiscardableEmptyAssistant', () => {
  it('keeps the active streaming shell that has not received tokens yet', () => {
    expect(
      isDiscardableEmptyAssistant(msg({ status: 'streaming', contentStreaming: true, content: '' }))
    ).toBe(false)
  })

  it('hides an empty shell after message_end cleared contentStreaming', () => {
    expect(
      isDiscardableEmptyAssistant(msg({ status: 'streaming', contentStreaming: false, content: '' }))
    ).toBe(true)
  })
})
