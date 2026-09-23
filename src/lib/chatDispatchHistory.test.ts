import { describe, expect, it } from 'vitest'
import {
  conversationNeedsHydration,
  messagesForChatDispatch,
  messagesForPersistAppend,
  persistedCandidateMessageIds
} from './chatDispatchHistory'
import type { ChatMessage } from '../types/chat'

describe('messagesForChatDispatch', () => {
  it('drops queued user messages and streaming assistant rows', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 },
      { id: 'a1', role: 'assistant', content: 'partial', status: 'streaming', createdAt: 2 },
      { id: 'u2', role: 'user', content: 'queued', status: 'pending', createdAt: 3 }
    ]
    const out = messagesForChatDispatch(messages)
    expect(out.map(m => m.id)).toEqual(['u1'])
  })

  it('when hydrated, only clones messages not yet persisted', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'old', status: 'done', createdAt: 1 },
      { id: 'a1', role: 'assistant', content: 'reply', status: 'done', createdAt: 2 },
      { id: 'u2', role: 'user', content: 'new', status: 'done', createdAt: 3 }
    ]
    const out = messagesForChatDispatch(messages, {
      persistedIds: new Set(['u1', 'a1'])
    })
    expect(out.map(m => m.id)).toEqual(['u2'])
    expect(out[0]).not.toBe(messages[2])
    expect(out[0]!.content).toBe('new')
  })

  it('falls through to an empty list when every dispatchable row is persisted', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 }
    ]
    expect(
      messagesForChatDispatch(messages, { persistedIds: new Set(['u1']) })
    ).toEqual([])
  })
})

describe('messagesForPersistAppend', () => {
  it('when hydrated, only clones messages not yet persisted', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'old', status: 'done', createdAt: 1 },
      { id: 'a1', role: 'assistant', content: 'reply', status: 'done', createdAt: 2 },
      { id: 'u2', role: 'user', content: 'new', status: 'done', createdAt: 3 }
    ]
    const out = messagesForPersistAppend(messages, {
      persistedIds: new Set(['u1', 'a1'])
    })
    expect(out.map(m => m.id)).toEqual(['u2'])
    expect(out[0]).not.toBe(messages[2])
  })

  it('skips pending outbound rows and returns empty when everything is persisted', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 },
      { id: 'u2', role: 'user', content: 'q', status: 'pending', createdAt: 2 }
    ]
    expect(
      messagesForPersistAppend(messages, { persistedIds: new Set(['u1']) })
    ).toEqual([])
  })

  it('without watermark clones all non-pending rows', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 },
      { id: 'u2', role: 'user', content: 'q', status: 'pending', createdAt: 2 }
    ]
    expect(messagesForPersistAppend(messages).map(m => m.id)).toEqual(['u1'])
  })
})

describe('persistedCandidateMessageIds', () => {
  it('skips pending outbound rows', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 },
      { id: 'u2', role: 'user', content: 'q', status: 'pending', createdAt: 2 }
    ]
    expect(persistedCandidateMessageIds(messages)).toEqual(['u1'])
  })
})

describe('conversationNeedsHydration', () => {
  it('blocks a persisted shell even when partial in-memory rows exist', () => {
    expect(
      conversationNeedsHydration({
        messageCount: 20,
        messagesLength: 1,
        hydrated: false,
        loading: false
      })
    ).toBe(true)
  })

  it('allows new and fully hydrated conversations', () => {
    expect(
      conversationNeedsHydration({
        messageCount: 0,
        messagesLength: 1,
        hydrated: false,
        loading: false
      })
    ).toBe(false)
    expect(
      conversationNeedsHydration({
        messageCount: 20,
        messagesLength: 20,
        hydrated: true,
        loading: false
      })
    ).toBe(false)
  })

  it('reloads when meta says rows exist but the in-memory list is empty', () => {
    expect(
      conversationNeedsHydration({
        messageCount: 6,
        messagesLength: 0,
        hydrated: true,
        loading: false
      })
    ).toBe(true)
  })
})

describe('rawContent wire stripping', () => {
  it('messagesForChatDispatch strips rawContent but keeps reasoning', () => {
    const messages: ChatMessage[] = [
      {
        id: 'a1',
        role: 'assistant',
        content: 'hello',
        status: 'done',
        createdAt: 1,
        toolCalls: [],
        rawContent: 'hello\n\nMEDIA:/tmp/x.png',
        reasoning: 'thinking…'
      }
    ]
    const out = messagesForChatDispatch(messages)
    expect(out).toHaveLength(1)
    expect(out[0].rawContent).toBeUndefined()
    // reasoning must survive: DeepSeek-style models require reasoning_content round-trip.
    expect(out[0].reasoning).toBe('thinking…')
    expect(out[0].content).toBe('hello')
  })

  it('messagesForPersistAppend strips rawContent but keeps reasoning', () => {
    const messages: ChatMessage[] = [
      {
        id: 'a1',
        role: 'assistant',
        content: 'hello',
        status: 'done',
        createdAt: 1,
        toolCalls: [],
        rawContent: 'hello\n\nMEDIA:/tmp/x.png',
        reasoning: 'thinking…'
      }
    ]
    const out = messagesForPersistAppend(messages)
    expect(out).toHaveLength(1)
    expect(out[0].rawContent).toBeUndefined()
    expect(out[0].reasoning).toBe('thinking…')
  })

  it('strips memory-only tool body flags before persist', () => {
    const messages: ChatMessage[] = [
      {
        id: 'a1',
        role: 'assistant',
        content: 'edited',
        status: 'done',
        createdAt: 1,
        toolCalls: [
          {
            id: 'tc1',
            name: 'file_edit',
            arguments: '',
            status: 'success',
            bodyEvicted: true,
            fileChange: { path: 'a.ts', kind: 'edit', adds: 1, dels: 0 }
          }
        ]
      }
    ]
    const out = messagesForPersistAppend(messages)
    const tc = out[0].toolCalls?.[0]
    expect(tc?.bodyEvicted).toBeUndefined()
    expect(tc?.fileChange).toBeUndefined()
    expect(tc?.arguments).toBe('')
  })

  it('strips nested agentTrace session rawContent too', () => {
    const messages: ChatMessage[] = [
      {
        id: 'a1',
        role: 'assistant',
        content: 'delegating',
        status: 'done',
        createdAt: 1,
        toolCalls: [],
        rawContent: 'delegating\n\nMEDIA:/tmp/x.png',
        agentTrace: [
          {
            id: 'trace-1',
            name: 'coder',
            role: 'worker',
            status: 'done',
            session: {
              rawContent: 'sub raw wire',
              reasoning: 'sub thinking',
              stats: { searchCount: 0, readCount: 0 },
              collapsed: false,
              userExpanded: false
            }
          }
        ]
      }
    ]
    const out = messagesForPersistAppend(messages)
    expect(out[0].rawContent).toBeUndefined()
    expect(out[0].agentTrace?.[0]?.session?.rawContent).toBeUndefined()
    // non-debug session fields survive
    expect(out[0].agentTrace?.[0]?.session?.reasoning).toBe('sub thinking')
    expect(out[0].agentTrace?.[0]?.name).toBe('coder')
  })
})
