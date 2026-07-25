import type { Ref } from 'vue'
import type { ToolCall } from '../../types/chat'
import { toolCallBaseName } from '../../lib/messageTooling'
import { parseTerminalCommandFromArgs } from '../../lib/terminalCommand'

const TERMINAL_LIVE_DELAY_MS = 5000

export interface TerminalLivePopup {
  messageId: string
  toolCallId: string
  traceId?: string
  command: string
  output: string
}

interface TerminalLiveTrack {
  messageId: string
  toolCallId: string
  traceId?: string
  /** Stream-scoped write target when the tool lives under a sub-agent message. */
  scopedMessageId?: string
  command: string
  delayTimer: ReturnType<typeof setTimeout> | null
  delayElapsed: boolean
}

function hasTerminalLiveOutput(output?: string): boolean {
  return (output ?? '').trim().length > 0
}

function terminalLiveKey(messageId: string, toolCallId: string, traceId?: string): string {
  return `${messageId}\u{1f}${toolCallId}\u{1f}${traceId?.trim() ?? ''}`
}

export interface TerminalLiveManager {
  clear(): void
  dismiss(): void
  /** User-initiated open after the 5s delay (no auto-popup). */
  open(toolCallId: string): void
  handleToolCallStatus(
    messageId: string,
    toolCallId: string,
    status: ToolCall['status'],
    traceId?: string,
    scopedMessageId?: string
  ): void
  syncPopupOutput(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ): void
}

export function createTerminalLiveManager(deps: {
  popup: Ref<TerminalLivePopup | null>
  /** Set to the tracked toolCallId once the 5s delay has elapsed; null otherwise. */
  viewReadyToolCallId: Ref<string | null>
  resolveToolCall: (
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ) => ToolCall | null
  delayMs?: number
}): TerminalLiveManager {
  let track: TerminalLiveTrack | null = null
  const delayMs = deps.delayMs ?? TERMINAL_LIVE_DELAY_MS

  function clear() {
    if (track?.delayTimer) clearTimeout(track.delayTimer)
    track = null
    deps.popup.value = null
    deps.viewReadyToolCallId.value = null
  }

  function dismiss() {
    deps.popup.value = null
  }

  function markViewReadyIfEligible() {
    if (!track || !track.delayElapsed) {
      deps.viewReadyToolCallId.value = null
      return
    }
    const current = deps.resolveToolCall(
      track.messageId,
      track.toolCallId,
      track.traceId,
      track.scopedMessageId
    )
    if (
      !current
      || current.status !== 'running'
      || current.waitingForInput
      || !hasTerminalLiveOutput(current.terminalOutput)
    ) {
      deps.viewReadyToolCallId.value = null
      return
    }
    deps.viewReadyToolCallId.value = track.toolCallId
  }

  function showFromTrack() {
    if (!track || !track.delayElapsed) return
    const current = deps.resolveToolCall(
      track.messageId,
      track.toolCallId,
      track.traceId,
      track.scopedMessageId
    )
    if (!current || current.status !== 'running') return
    if (current.waitingForInput) return
    if (!hasTerminalLiveOutput(current.terminalOutput)) return
    deps.popup.value = {
      messageId: track.messageId,
      toolCallId: track.toolCallId,
      traceId: track.traceId,
      command: track.command,
      output: current.terminalOutput ?? ''
    }
  }

  function open(toolCallId: string) {
    const id = toolCallId.trim()
    if (!id) {
      console.warn('[terminalLive] open ignored: empty toolCallId')
      return
    }
    if (!track || track.toolCallId !== id) {
      console.warn('[terminalLive] open ignored: no active track for toolCallId', id)
      return
    }
    if (!track.delayElapsed) {
      console.warn('[terminalLive] open ignored: 5s delay not elapsed yet', id)
      return
    }
    showFromTrack()
  }

  function syncPopupOutput(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ) {
    markViewReadyIfEligible()
    const popup = deps.popup.value
    if (!popup) return
    if (terminalLiveKey(messageId, toolCallId, traceId) !== terminalLiveKey(
      popup.messageId,
      popup.toolCallId,
      popup.traceId
    )) {
      return
    }
    const tc = deps.resolveToolCall(messageId, toolCallId, traceId, scopedMessageId)
    if (!tc) return
    deps.popup.value = { ...popup, output: tc.terminalOutput ?? '' }
  }

  function beginTrack(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ) {
    const tc = deps.resolveToolCall(messageId, toolCallId, traceId, scopedMessageId)
    if (!tc || toolCallBaseName(tc.name) !== 'terminal') return

    const key = terminalLiveKey(messageId, toolCallId, traceId)
    if (
      track &&
      terminalLiveKey(track.messageId, track.toolCallId, track.traceId) === key
    ) {
      return
    }

    clear()

    const nextTrack: TerminalLiveTrack = {
      messageId,
      toolCallId,
      traceId: traceId?.trim() || undefined,
      scopedMessageId: scopedMessageId?.trim() || undefined,
      command: parseTerminalCommandFromArgs(tc.arguments),
      delayTimer: null,
      delayElapsed: false
    }
    track = nextTrack

    nextTrack.delayTimer = setTimeout(() => {
      if (!track) return
      if (terminalLiveKey(track.messageId, track.toolCallId, track.traceId) !== key) {
        return
      }
      track.delayElapsed = true
      markViewReadyIfEligible()
    }, delayMs)
  }

  function finishTrack(messageId: string, toolCallId: string, traceId?: string) {
    if (!track) return
    if (terminalLiveKey(messageId, toolCallId, traceId) !== terminalLiveKey(
      track.messageId,
      track.toolCallId,
      track.traceId
    )) {
      return
    }
    clear()
  }

  function handleToolCallStatus(
    messageId: string,
    toolCallId: string,
    status: ToolCall['status'],
    traceId?: string,
    scopedMessageId?: string
  ) {
    const tc = deps.resolveToolCall(messageId, toolCallId, traceId, scopedMessageId)
    if (!tc || toolCallBaseName(tc.name) !== 'terminal') return
    if (status === 'running') {
      beginTrack(messageId, toolCallId, traceId, scopedMessageId)
      return
    }
    if (status === 'success' || status === 'failed' || status === 'rejected') {
      finishTrack(messageId, toolCallId, traceId)
    }
  }

  return {
    clear,
    dismiss,
    open,
    handleToolCallStatus,
    syncPopupOutput
  }
}
