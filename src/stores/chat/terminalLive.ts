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
  command: string
  delayTimer: ReturnType<typeof setTimeout> | null
  delayElapsed: boolean
  dismissed: boolean
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
  }

  function dismiss() {
    if (!deps.popup.value) return
    deps.popup.value = null
    if (track) track.dismissed = true
  }

  function tryShowPopup(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ) {
    if (!track || track.dismissed || !track.delayElapsed) return
    if (terminalLiveKey(messageId, toolCallId, traceId) !== terminalLiveKey(
      track.messageId,
      track.toolCallId,
      track.traceId
    )) {
      return
    }
    if (deps.popup.value) return
    const current = deps.resolveToolCall(messageId, toolCallId, traceId, scopedMessageId)
    if (!current || current.status !== 'running') return
    if (!hasTerminalLiveOutput(current.terminalOutput)) return
    deps.popup.value = {
      messageId,
      toolCallId,
      traceId: traceId?.trim() || undefined,
      command: track.command,
      output: current.terminalOutput ?? ''
    }
  }

  function syncPopupOutput(
    messageId: string,
    toolCallId: string,
    traceId?: string,
    scopedMessageId?: string
  ) {
    const popup = deps.popup.value
    if (!popup) {
      tryShowPopup(messageId, toolCallId, traceId, scopedMessageId)
      return
    }
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
      command: parseTerminalCommandFromArgs(tc.arguments),
      delayTimer: null,
      delayElapsed: false,
      dismissed: false
    }
    track = nextTrack

    nextTrack.delayTimer = setTimeout(() => {
      if (!track) return
      if (terminalLiveKey(track.messageId, track.toolCallId, track.traceId) !== key) {
        return
      }
      if (track.dismissed) return
      track.delayElapsed = true
      tryShowPopup(messageId, toolCallId, traceId, scopedMessageId)
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
    handleToolCallStatus,
    syncPopupOutput
  }
}
