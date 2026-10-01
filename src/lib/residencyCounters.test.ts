// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatMessage, ComposerAttachment } from '../types/chat'
import {
  composerAttachmentPayloadCount,
  registerComposerAttachmentPayload,
  releaseComposerAttachment
} from './attachmentPayloadStore'
import { createConversationScopedStore } from './conversationScoped/store'
import { canvasRgb, canvasRgbCacheSize } from './markdownChart'
import {
  markdownParseCacheRetainedChars,
  markdownParseCacheSize,
  parseMarkdown
} from './markdownConfig'
import {
  clearContentDeltaBuffer,
  clearReasoningDeltaBuffer,
  enqueueContentDelta,
  enqueueReasoningDelta,
  pendingDeltaCount
} from './reasoningDeltaBatch'
import {
  clearAllTurnExpandUiState,
  loadTurnExpandUiState,
  turnExpandConversationCount
} from './turnExpandState'
import { markdownMermaidCacheSize } from '../composables/useMarkdownMermaid'
import { markdownSvgCacheSize } from '../composables/useMarkdownSvgs'

/**
 * The residency accessors the HUD's panel reads (`lib/residencyProbe.ts`).
 *
 * Each case drives a cache through its own public write path and asserts the
 * accessor follows its container — the point of the panel is that a number it
 * shows is the size of the thing, not a copy that can drift from it.
 */

function scopedRow(id: string): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    anchorMessageId: 'anchor-1',
    agentInstanceId: 'inst-1',
    traceId: 'task:explore'
  }
}

beforeEach(() => {
  // The delta enqueues arm flush timers; nothing here advances them.
  vi.useFakeTimers()
  clearAllTurnExpandUiState()
})

afterEach(() => {
  clearReasoningDeltaBuffer()
  clearContentDeltaBuffer()
  clearAllTurnExpandUiState()
  vi.useRealTimers()
})

describe('markdown parse cache accessor', () => {
  it('reports the entries and the source characters the cache holds', () => {
    const entries = markdownParseCacheSize()
    const sourceChars = markdownParseCacheRetainedChars()
    const source = '**bold**'

    parseMarkdown(source)
    expect(markdownParseCacheSize()).toBe(entries + 1)
    expect(markdownParseCacheRetainedChars()).toBe(sourceChars + source.length)

    // A hit re-uses the entry rather than adding one.
    parseMarkdown(source)
    expect(markdownParseCacheSize()).toBe(entries + 1)
    expect(markdownParseCacheRetainedChars()).toBe(sourceChars + source.length)
  })
})

describe('canvas colour cache accessor', () => {
  it('reports the resolved colours it holds, once each', () => {
    const before = canvasRgbCacheSize()

    canvasRgb('--probe-colour', '0 0% 0%')
    expect(canvasRgbCacheSize()).toBe(before + 1)

    canvasRgb('--probe-colour', '0 0% 0%')
    expect(canvasRgbCacheSize()).toBe(before + 1)
  })
})

describe('pending delta accessor', () => {
  it('counts the entries buffered across the pending maps', () => {
    const before = pendingDeltaCount()

    enqueueReasoningDelta('message-1', 'thinking')
    enqueueContentDelta('message-1', 'body')
    expect(pendingDeltaCount()).toBe(before + 2)

    clearReasoningDeltaBuffer()
    clearContentDeltaBuffer()
    expect(pendingDeltaCount()).toBe(before)
  })
})

describe('attachment payload accessor', () => {
  it('counts the payloads still held, and follows a release', () => {
    const before = composerAttachmentPayloadCount()
    const attachment: ComposerAttachment = {
      id: 'attachment-1',
      kind: 'image',
      mimeType: 'image/png',
      fileName: 'shot.png',
      sizeBytes: 3
    }

    registerComposerAttachmentPayload({
      attachment,
      dataUrl: 'data:image/png;base64,AAA',
      contentBase64: 'AAA'
    })
    expect(composerAttachmentPayloadCount()).toBe(before + 1)

    releaseComposerAttachment(attachment.id)
    expect(composerAttachmentPayloadCount()).toBe(before)
  })
})

describe('turn expand state accessor', () => {
  it('counts the conversations whose expand state is retained', () => {
    expect(turnExpandConversationCount()).toBe(0)

    // A miss hydrates the conversation into memory, which is what retains it.
    loadTurnExpandUiState('conversation-a')
    expect(turnExpandConversationCount()).toBe(1)

    clearAllTurnExpandUiState()
    expect(turnExpandConversationCount()).toBe(0)
  })
})

describe('scoped row accessors', () => {
  it('count the live rows and the spawn transcripts of one conversation', () => {
    const store = createConversationScopedStore()

    expect(store.countRows('conversation-a')).toBe(0)

    store.ingestRows('conversation-a', [scopedRow('row-1'), scopedRow('row-2')])
    expect(store.countRows('conversation-a')).toBe(2)
    expect(store.countSpawns('conversation-a')).toBe(1)

    // Another conversation's rows are not folded in.
    expect(store.countRows('conversation-b')).toBe(0)
  })
})

describe('cleaned diagram cache accessors', () => {
  it('report the entries of their own module-level map', () => {
    // Filling these needs a mounted markdown host, so this pins the empty state;
    // `residencyProbe.test.ts` asserts the probe publishes these accessors.
    expect(markdownSvgCacheSize()).toBe(0)
    expect(markdownMermaidCacheSize()).toBe(0)
  })
})
