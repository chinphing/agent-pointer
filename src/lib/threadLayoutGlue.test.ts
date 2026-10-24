// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  glueMessagePreview,
  isInternalRetryUserMessage,
  isRealUserTaskMessage,
  isSilentToolRunGlue,
  isSyntheticThreadUserMessage,
  isToolRunContinuityGlue
} from './threadLayoutGlue'

function user(id: string, content: string): ChatMessage {
  return { id, role: 'user', content, status: 'done', createdAt: 1 }
}

function assistant(
  id: string,
  content: string,
  toolCalls: ChatMessage['toolCalls'] = []
): ChatMessage {
  return { id, role: 'assistant', content, status: 'done', createdAt: 2, toolCalls }
}

describe('isInternalRetryUserMessage', () => {
  it('matches empty-response and env-feedback injects', () => {
    expect(
      isInternalRetryUserMessage(
        user(
          'fmt_retry_abc',
          '你的上一次回复为空，既没有文本内容也没有工具调用。（异常重试 1/3）'
        )
      )
    ).toBe(true)
    expect(
      isInternalRetryUserMessage(user('u1', '【环境反馈】上游服务暂时不可用，正在重试。'))
    ).toBe(true)
    expect(isInternalRetryUserMessage(user('u2', '【输出长度】本回合因输出 token 上限被截断'))).toBe(
      true
    )
  })

  it('does not match real user text', () => {
    expect(isInternalRetryUserMessage(user('u3', '帮我写一份周报'))).toBe(false)
    expect(isInternalRetryUserMessage(user('u4', '[CUR_SCREEN]\nshot'))).toBe(false)
  })
})

describe('synthetic thread user rows', () => {
  it('hides internal retries as silent glue', () => {
    const retry = user(
      'fmt_retry_1',
      '你的上一次回复为空，既没有文本内容也没有工具调用。请重新处理用户请求。（异常重试 1/3）'
    )
    expect(isSyntheticThreadUserMessage(retry)).toBe(true)
    expect(isRealUserTaskMessage(retry)).toBe(false)
    expect(isSilentToolRunGlue(retry)).toBe(true)
  })

  it('keeps real user turns visible', () => {
    const real = user('u5', '继续排查终端中文输入')
    expect(isSyntheticThreadUserMessage(real)).toBe(false)
    expect(isRealUserTaskMessage(real)).toBe(true)
    expect(isSilentToolRunGlue(real)).toBe(false)
  })

  it('does not treat compression chips as user-task turns', () => {
    const chip = user('ctx_1', '[Conversation summary (auto-compression)]\nbody')
    expect(isRealUserTaskMessage(chip)).toBe(false)
  })
})

describe('sidecar-only assistant rounds', () => {
  const sidecarCalls: ChatMessage['toolCalls'] = [
    { id: 'tc-tb', name: 'task_board_init', status: 'success', arguments: '{}' }
  ]

  it('keeps a round that carries reply text visible', () => {
    const round = assistant('a1', '好思路——先看提示词长什么样：', sidecarCalls)
    expect(isSilentToolRunGlue(round)).toBe(false)
    expect(isToolRunContinuityGlue(round)).toBe(false)
  })

  it('still hides a sidecar round with no reply text', () => {
    const round = assistant('a2', '', sidecarCalls)
    expect(isSilentToolRunGlue(round)).toBe(true)
    expect(isToolRunContinuityGlue(round)).toBe(true)
  })

  it('still hides verify-hint rounds even with text', () => {
    const round = assistant('a3', 'Attempted click at coordinates (10, 20)')
    expect(isSilentToolRunGlue(round)).toBe(true)
  })
})

describe('glueMessagePreview', () => {
  it('renders nothing for an empty glue row', () => {
    // Glue rows exist to keep consecutive tool cards in one block. An empty one
    // has no text of its own — returning `…` made it look like truncated
    // content and painted a stray ellipsis between every tool row.
    expect(glueMessagePreview(assistant('a1', ''))).toBe('')
    expect(glueMessagePreview(user('u1', ''))).toBe('')
  })

  it('keeps the tool-result placeholder for tool rows', () => {
    const row: ChatMessage = { id: 't1', role: 'tool', content: '', status: 'done', createdAt: 1 }
    expect(glueMessagePreview(row)).toBe('工具结果')
  })

  it('uses the first non-empty line and truncates long previews', () => {
    expect(glueMessagePreview(assistant('a2', '\n\n第一行\n第二行'))).toBe('第一行')
    const long = 'x'.repeat(200)
    const out = glueMessagePreview(assistant('a3', long))
    expect(out.endsWith('…')).toBe(true)
    expect(out.length).toBe(118)
  })

  it('labels screen inject rows as 桌面截图', () => {
    expect(glueMessagePreview(user('u2', '[CUR_SCREEN] whatever'))).toBe('桌面截图')
  })
})
