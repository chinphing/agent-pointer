import type { ChatMessage } from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'

export interface ComputerUiSettings {
  agentMode: 'single' | 'supervisor'
  leadAgentId: string
  annotatedScreenViewEnabled: boolean
}

export function computerSingleLead(settings: ComputerUiSettings): boolean {
  if (settings.agentMode !== 'single') return false
  const id = settings.leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  return id === 'computer'
}

export function messageFromComputerAgent(message: ChatMessage): boolean {
  if (message.agentId === 'computer') return true
  return message.agentTrace?.some(t => t.id === 'computer') ?? false
}

export function showAnnotatedScreenAction(
  message: ChatMessage,
  settings: ComputerUiSettings
): boolean {
  if (!settings.annotatedScreenViewEnabled) return false
  if (message.role !== 'assistant') return false
  if (computerSingleLead(settings)) return true
  if (settings.agentMode === 'supervisor' && messageFromComputerAgent(message)) return true
  return false
}
