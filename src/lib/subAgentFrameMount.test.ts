import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage } from '../types/chat'
import {
  persistTerminalTraceSummaryLine,
  shouldKeepFullSubAgentFrame,
  shouldRenderTerminalSubAgentStub,
  shouldStubTerminalImmediately,
  TERMINAL_COLLAPSED_STUB_IDLE_MS
} from './subAgentFrameMount'

function trace(over: Partial<AgentTrace> & { id: string }): AgentTrace {
  return {
    name: 'explore',
    role: 'assistant',
    status: 'completed',
    collapsed: true,
    userExpanded: false,
    ...over
  }
}

describe('subAgentFrameMount', () => {
  it('keeps full frame while running', () => {
    expect(shouldKeepFullSubAgentFrame(trace({ id: 't1', status: 'running' }))).toBe(true)
  })

  it('keeps full frame when user expanded', () => {
    expect(
      shouldKeepFullSubAgentFrame(trace({ id: 't1', status: 'completed', userExpanded: true }))
    ).toBe(true)
  })

  it('allows stub after idle for terminal collapsed traces', () => {
    const t = trace({ id: 't1', status: 'completed' })
    expect(shouldRenderTerminalSubAgentStub(t, false)).toBe(false)
    expect(shouldRenderTerminalSubAgentStub(t, true)).toBe(true)
  })

  it('never stubs running traces even after idle', () => {
    const t = trace({ id: 't1', status: 'running' })
    expect(shouldRenderTerminalSubAgentStub(t, true)).toBe(false)
  })

  it('remount stays collapsed until userExpanded', () => {
    const t = trace({ id: 't1', status: 'completed', userExpanded: false })
    expect(shouldRenderTerminalSubAgentStub(t, true)).toBe(true)
    expect(shouldKeepFullSubAgentFrame({ ...t, userExpanded: false })).toBe(false)
    expect(shouldKeepFullSubAgentFrame({ ...t, userExpanded: true })).toBe(true)
  })

  it('exports a positive idle delay', () => {
    expect(TERMINAL_COLLAPSED_STUB_IDLE_MS).toBeGreaterThan(0)
  })

  it('stubs hydrated terminal traces immediately (no grace)', () => {
    expect(
      shouldStubTerminalImmediately({
        initialized: false,
        status: 'completed'
      })
    ).toBe(true)
  })

  it('uses grace for in-session terminal collapse (finish or re-collapse)', () => {
    expect(
      shouldStubTerminalImmediately({
        initialized: true,
        status: 'completed'
      })
    ).toBe(false)
  })

  it('persists search tool-call ids for stub pin after evict', () => {
    const t = trace({ id: 'inst-1', status: 'completed' })
    const scoped: ChatMessage[] = [{
      id: 'm1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      anchorMessageId: 'lead',
      toolCalls: [{ id: 'tc-hit', name: 'read', arguments: '{}', status: 'success' }]
    }]
    persistTerminalTraceSummaryLine(t, { scopedTraceMessages: scoped })
    expect(t.searchToolCallIds).toEqual(['tc-hit'])
  })

  it('overwrites a stale summaryLine once scoped rows are available', () => {
    const t = trace({
      id: 'inst-1',
      status: 'completed',
      summaryLine: 'explore',
      agentId: 'explore'
    })
    const scoped: ChatMessage[] = [{
      id: 'm1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      anchorMessageId: 'lead',
      toolCalls: [{ id: 'tc-1', name: 'file_read', arguments: '{}', status: 'success' }]
    }]
    persistTerminalTraceSummaryLine(t, { scopedTraceMessages: scoped })
    expect(t.summaryLine).toContain('读文件')
    expect(t.summaryLine).not.toBe('explore')
  })

  it('falls back to session stats when scoped rows are empty', () => {
    const t = trace({
      id: 'inst-1',
      status: 'completed',
      agentId: 'explore',
      session: {
        stats: {
          searchCount: 2,
          readCount: 3,
          writeCount: 0,
          terminalCount: 0,
          webSearchCount: 0,
          skillCount: 0,
          mediaCount: 0,
          mouseCount: 0,
          inputCount: 0,
          otherCount: 0
        },
        collapsed: true,
        userExpanded: false
      }
    })
    persistTerminalTraceSummaryLine(t, { scopedTraceMessages: [] })
    expect(t.summaryLine).toContain('搜索 2 次')
    expect(t.summaryLine).toContain('读文件 3 次')
  })
})
