// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  isInternalRetryUserMessage,
  isRealUserTaskMessage,
  isSilentToolRunGlue,
  isSyntheticThreadUserMessage
} from './threadLayoutGlue'

function user(id: string, content: string): ChatMessage {
  return { id, role: 'user', content, status: 'done', createdAt: 1 }
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
})
