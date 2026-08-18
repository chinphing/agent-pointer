import type { StreamEvent } from '../../../types/chat'
import { handleAgentStep } from './agentHandlers'
import {
  handleContextCompressed,
  handleContextCompressionApplied,
  handleContextCompressionStarted,
  handleContextTrimApplied
} from './contextHandlers'
import { handleSubMessageStart } from './subMessageHandlers'
import {
  handleAssistantJsonPartial,
  handleDelta,
  handleMessageEnd,
  handleMessageStart,
  handleRawContentDelta,
  handleReasoningDelta
} from './messageHandlers'
import {
  handleAssistantRoundScreen,
  handleComputerMonitorPickRequired,
  handleComputerMonitorUpdated,
  handleDone,
  handleChannelPairingPending,
  handleImSessionAgentChanged,
  handleImSessionForked,
  handleInjectedAssistantMessage,
  handleInjectedAssistantMessageUpdate,
  handleInjectedUserMessage,
  handleSkillsUpdated,
  handleStreamError,
  handleToolRoundsExhausted,
  handleUiToast,
  handleUserMessageAttachmentsUpdated,
  handleWorkspaceUpdated
} from './sessionHandlers'
import {
  handleTaskBoardUpdated,
  handleTerminalOutputDelta,
  handleTerminalNeedsInput,
  handleToolCallArgsDelta,
  handleToolCallStart,
  handleToolCallStatus,
  handleWebSearchOutputDelta,
  handleWebSearchSourcesReady
} from './toolHandlers'
import type { StreamHandlerContext } from './types'

export function dispatchStreamEvent(ctx: StreamHandlerContext, e: StreamEvent): void {
  switch (e.kind) {
    case 'context_trim_applied':
      handleContextTrimApplied(ctx, e)
      break
    case 'context_compression_started':
      handleContextCompressionStarted(ctx, e)
      break
    case 'context_compression_applied':
      handleContextCompressionApplied(ctx, e)
      break
    case 'context_compressed':
      handleContextCompressed(ctx, e)
      break
    case 'ui_toast':
      handleUiToast(ctx, e)
      break
    case 'channel_pairing_pending':
      handleChannelPairingPending(e)
      break
    case 'tool_rounds_exhausted':
      handleToolRoundsExhausted(ctx, e)
      break
    case 'sub_message_start':
      handleSubMessageStart(ctx, e)
      break
    case 'message_start':
      handleMessageStart(ctx, e)
      break
    case 'delta':
      handleDelta(ctx, e)
      break
    case 'raw_content_delta':
      handleRawContentDelta(ctx, e)
      break
    case 'reasoning_delta':
      handleReasoningDelta(ctx, e)
      break
    case 'assistant_json_partial':
      handleAssistantJsonPartial(ctx, e)
      break
    case 'agent_step':
      handleAgentStep(ctx, e)
      break
    case 'task_board_updated':
      handleTaskBoardUpdated(ctx, e)
      break
    case 'skills_updated':
      handleSkillsUpdated(ctx, e)
      break
    case 'workspace_updated':
      handleWorkspaceUpdated(ctx, e)
      break
    case 'computer_monitor_pick_required':
      handleComputerMonitorPickRequired(ctx, e)
      break
    case 'computer_monitor_updated':
      handleComputerMonitorUpdated(ctx, e)
      break
    case 'tool_call_start':
      handleToolCallStart(ctx, e)
      break
    case 'tool_call_args_delta':
      handleToolCallArgsDelta(ctx, e)
      break
    case 'tool_call_status':
      handleToolCallStatus(ctx, e)
      break
    case 'terminal_output_delta':
      handleTerminalOutputDelta(ctx, e)
      break
    case 'console_output_delta':
    case 'console_session_exited':
      // Persistent workspace console owns these events in useConsoleStore;
      // they are intentionally isolated from chat message state.
      break
    case 'terminal_needs_input':
      handleTerminalNeedsInput(ctx, e)
      break
    case 'web_search_output_delta':
      handleWebSearchOutputDelta(ctx, e)
      break
    case 'web_search_sources_ready':
      handleWebSearchSourcesReady(ctx, e)
      break
    case 'message_end':
      handleMessageEnd(ctx, e)
      break
    case 'im_session_forked':
      handleImSessionForked(ctx, e)
      break
    case 'im_session_agent_changed':
      handleImSessionAgentChanged(ctx, e)
      break
    case 'user_message_attachments_updated':
      handleUserMessageAttachmentsUpdated(ctx, e)
      break
    case 'injected_user_message':
      handleInjectedUserMessage(ctx, e)
      break
    case 'injected_assistant_message':
      handleInjectedAssistantMessage(ctx, e)
      break
    case 'injected_assistant_message_update':
      handleInjectedAssistantMessageUpdate(ctx, e)
      break
    case 'assistant_round_screen':
      handleAssistantRoundScreen(ctx, e)
      break
    case 'error':
      handleStreamError(ctx, e)
      break
    case 'done':
      handleDone(ctx, e)
      break
    default: {
      const _exhaustive: never = e
      console.warn('[chat stream] unhandled event kind', _exhaustive)
    }
  }
}

export type { StreamHandlerContext } from './types'
