import { describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import { compactToolCallStatusLine, latestToolCallForCompactStatus, resolveToolDisplayForCall } from './toolCallDisplay'

function tc(partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name' | 'status'>): ToolCall {
  return {
    arguments: '',
    ...partial
  }
}

describe('compactToolCallStatusLine', () => {
  it('formats running tool like expanded ToolCallRow', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '1',
        name: 'mouse',
        status: 'running',
        displayLabel: '鼠标',
        displaySummary: '点击微信图标'
      })
    )
    expect(line).toBe('鼠标 · 点击微信图标 · 执行中')
  })

  it('formats success tool without duration suffix', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '2',
        name: 'mouse',
        status: 'success',
        displayLabel: '鼠标',
        displaySummary: '点击微信图标',
        durationMs: 1926
      })
    )
    expect(line).toBe('鼠标 · 点击微信图标')
  })

  it('latestToolCallForCompactStatus prefers in-progress over completed', () => {
    const calls = [
      tc({ id: '1', name: 'mouse', status: 'success', displayLabel: '鼠标' }),
      tc({ id: '2', name: 'wait', status: 'running', displayLabel: '等待' })
    ]
    expect(latestToolCallForCompactStatus(calls)?.displayLabel).toBe('等待')
    expect(latestToolCallForCompactStatus([calls[0]])?.displayLabel).toBe('鼠标')
  })

  it('falls back display for launch_app without backend labels', () => {
    const d = resolveToolDisplayForCall(
      tc({
        id: '3',
        name: 'launch_app',
        status: 'success',
        arguments: JSON.stringify({ goal: '打开微信', app: 'WeChat' })
      })
    )
    expect(d.label).toBe('启动应用')
    expect(d.summary).toBe('WeChat')
  })
})
