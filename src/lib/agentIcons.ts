import { Bot, Code, Monitor, Search, type LucideIcon } from 'lucide-vue-next'
import type { AgentDef, ModelSettings } from '../types/chat'
import { resolveAgentUi } from './agentUi'

export const COMPOSER_AGENT_ORDER = ['general', 'coder', 'computer', 'explore'] as const

export function sortComposerAgents(agents: AgentDef[]): AgentDef[] {
  const rank = new Map<string, number>(COMPOSER_AGENT_ORDER.map((id, i) => [id, i]))
  return [...agents].sort((a, b) => {
    const ra = rank.get(a.id) ?? 999
    const rb = rank.get(b.id) ?? 999
    if (ra !== rb) return ra - rb
    return a.name.localeCompare(b.name)
  })
}

export function iconForAgentAvatar(avatar: string): LucideIcon {
  if (avatar === 'computer') return Monitor
  if (avatar === 'coder') return Code
  if (avatar === 'explore') return Search
  return Bot
}

export function iconForAgent(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): LucideIcon {
  if (!agent) return Bot
  const avatar = resolveAgentUi(agent, settings).avatar
  return iconForAgentAvatar(avatar)
}
