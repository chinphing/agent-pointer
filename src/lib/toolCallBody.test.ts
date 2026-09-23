import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import {
  TOOL_BODY_CHAR_LIMIT,
  TERMINAL_OUTPUT_TRUNCATED_PREFIX,
  capTerminalOutput,
  restoreFinishedAside,
  restoreToolCallBody,
  slimFinishedAside,
  slimMessageForMemory,
  slimToolCallBody
} from './toolCallBody'

function tool(partial: Partial<ToolCall> & Pick<ToolCall, 'id'>): ToolCall {
  return {
    name: 'file_read',
    arguments: '{}',
    status: 'success',
    ...partial
  }
}

describe('capTerminalOutput', () => {
  it('keeps a short stream and prefixes the tail once it exceeds the cap', () => {
    const limit = 40
    const first = capTerminalOutput('hello', ' world', limit)
    expect(first).toEqual({ text: 'hello world', newlyTruncated: false })

    const second = capTerminalOutput('a'.repeat(50), 'zzzz', limit)
    expect(second.newlyTruncated).toBe(true)
    expect(second.text.startsWith(TERMINAL_OUTPUT_TRUNCATED_PREFIX)).toBe(true)
    expect(second.text.endsWith('zzzz')).toBe(true)
    expect(second.text.length).toBe(limit)

    const third = capTerminalOutput(second.text, 'q', limit)
    expect(third.newlyTruncated).toBe(false)
    expect(third.text.startsWith(TERMINAL_OUTPUT_TRUNCATED_PREFIX)).toBe(true)
    expect(third.text.endsWith('q')).toBe(true)
    expect(third.text.length).toBe(limit)
  })
})

describe('slimToolCallBody', () => {
  it('keeps a short finished body and records that it was not evicted', () => {
    const tc = tool({ id: 'short', result: 'ok' })
    expect(slimToolCallBody(tc)).toBe(0)
    expect(tc.bodyEvicted).toBe(false)
    expect(tc.result).toBe('ok')
    expect(tc.arguments).toBe('{}')
  })

  it('clears the whole body group when any string is over the limit', () => {
    const big = 'x'.repeat(TOOL_BODY_CHAR_LIMIT + 1)
    const tc = tool({
      id: 'big',
      name: 'file_edit',
      arguments: JSON.stringify({ path: 'src/a.ts', old_string: big, new_string: 'y' }),
      result: JSON.stringify({
        success: true,
        path: 'src/a.ts',
        stats: { adds: 3, dels: 1 }
      }),
      terminalOutput: 'tail',
      webSearchOutput: 'search',
      displayLabel: '编辑文件'
    })
    const cleared = slimToolCallBody(tc)
    expect(cleared).toBeGreaterThan(TOOL_BODY_CHAR_LIMIT)
    expect(tc.bodyEvicted).toBe(true)
    expect(tc.arguments).toBe('')
    expect(tc.result).toBeUndefined()
    expect(tc.terminalOutput).toBeUndefined()
    expect(tc.webSearchOutput).toBeUndefined()
    expect(tc.displayLabel).toBe('编辑文件')
    expect(tc.fileChange).toEqual({ path: 'src/a.ts', kind: 'edit', adds: 3, dels: 1 })
    expect(tc.displaySummary?.length).toBeGreaterThan(0)
  })

  it('does not treat a second pass over an evicted call as a short body', () => {
    const tc = tool({
      id: 'again',
      arguments: '',
      bodyEvicted: true,
      fileChange: { path: 'src/a.ts', kind: 'edit', adds: 1, dels: 0 }
    })
    expect(slimToolCallBody(tc)).toBe(0)
    expect(tc.bodyEvicted).toBe(true)
    expect(tc.fileChange?.path).toBe('src/a.ts')
  })

  it('leaves a running call intact', () => {
    const tc = tool({
      id: 'run',
      status: 'running',
      arguments: 'x'.repeat(TOOL_BODY_CHAR_LIMIT + 8),
      terminalOutput: 'live'
    })
    expect(slimToolCallBody(tc)).toBe(0)
    expect(tc.bodyEvicted).toBeUndefined()
    expect(tc.terminalOutput).toBe('live')
  })
})

describe('slimMessageForMemory', () => {
  it('drops tool-role content and skips the pinned tool call', () => {
    const pinned = tool({ id: 'keep', result: 'y'.repeat(TOOL_BODY_CHAR_LIMIT + 4) })
    const other = tool({ id: 'drop', result: 'z'.repeat(TOOL_BODY_CHAR_LIMIT + 4) })
    const msg: ChatMessage = {
      id: 'm1',
      role: 'assistant',
      content: 'done',
      status: 'done',
      createdAt: 1,
      toolCalls: [pinned, other]
    }
    const toolRow: ChatMessage = {
      id: 'tool-1',
      role: 'tool',
      content: 'duplicate result',
      status: 'done',
      createdAt: 2,
      toolCallId: 'drop'
    }
    expect(slimMessageForMemory(msg, 'keep')).toBeGreaterThan(0)
    expect(pinned.result?.startsWith('yyyy')).toBe(true)
    expect(pinned.bodyEvicted).toBeUndefined()
    expect(other.bodyEvicted).toBe(true)
    expect(other.result).toBeUndefined()
    expect(slimMessageForMemory(toolRow)).toBe('duplicate result'.length)
    expect(toolRow.content).toBe('')
    expect(toolRow.toolCallId).toBe('drop')
  })
})

describe('slimFinishedAside', () => {
  it('clears reasoning and thoughts after the round and leaves content', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'visible reply',
      reasoning: 'think',
      thoughts: 'plan',
      status: 'done',
      createdAt: 1,
      contentStreaming: false
    }
    expect(slimFinishedAside(msg)).toBe('think'.length + 'plan'.length)
    expect(msg.reasoning).toBeUndefined()
    expect(msg.thoughts).toBeUndefined()
    expect(msg.content).toBe('visible reply')
    expect(msg.asideEvicted).toBe(true)
  })

  it('keeps the text while that round is still streaming', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: '',
      reasoning: 'think',
      thoughts: 'plan',
      status: 'streaming',
      createdAt: 1,
      contentStreaming: true
    }
    expect(slimFinishedAside(msg)).toBe(0)
    expect(msg.reasoning).toBe('think')
    expect(msg.thoughts).toBe('plan')
    expect(msg.asideEvicted).toBeUndefined()
  })

  it('clears text that arrives after an earlier eviction', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'reply',
      reasoning: 'again',
      status: 'done',
      createdAt: 1,
      asideEvicted: true
    }
    expect(slimFinishedAside(msg)).toBe('again'.length)
    expect(msg.reasoning).toBeUndefined()
    expect(msg.content).toBe('reply')
    expect(msg.asideEvicted).toBe(true)
  })

  it('copies the two fields back without touching content', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'visible',
      status: 'done',
      createdAt: 1,
      asideEvicted: true
    }
    restoreFinishedAside(msg, { reasoning: 'think', thoughts: 'plan' })
    expect(msg.reasoning).toBe('think')
    expect(msg.thoughts).toBe('plan')
    expect(msg.content).toBe('visible')
    expect(msg.asideEvicted).toBe(false)
  })
})

describe('restoreToolCallBody', () => {
  it('copies the body group back and clears the evicted flag', () => {
    const target = tool({ id: 't', arguments: '', bodyEvicted: true })
    restoreToolCallBody(target, tool({
      id: 't',
      arguments: '{"path":"a.ts"}',
      result: 'full',
      terminalOutput: 'out'
    }))
    expect(target.bodyEvicted).toBe(false)
    expect(target.arguments).toBe('{"path":"a.ts"}')
    expect(target.result).toBe('full')
    expect(target.terminalOutput).toBe('out')
    expect(target.webSearchOutput).toBeUndefined()
  })
})
