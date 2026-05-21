import type { AgentDef, AgentProfile, AgentUiConfig, ModelSettings } from '../types/chat'

export interface ResolvedAgentUi {
  showInComposer: boolean
  showAgentLabel: boolean
  showThoughts: boolean
  showHeadline: boolean
  showSubAgentTrace: boolean
  showToolCalls: boolean
  hideToolNames: string[]
  showWorkspacePicker: boolean
  showComputerMonitorPicker: boolean
  showTaskBoardPanel: boolean
  avatar: string
}

function profileKey(profile: AgentProfile, id: string, role: string): string {
  if (role === 'supervisor' || id === 'supervisor') return 'supervisor'
  if (typeof profile === 'string') {
    if (profile === 'computer' || id === 'computer') return 'computer'
    if (profile === 'coder' || id === 'coder') return 'coder'
    if (profile === 'explore' || id === 'explore') return 'explore'
  }
  return 'default'
}

function profileDefaults(profile: AgentProfile, id: string, role: string): ResolvedAgentUi {
  const key = profileKey(profile, id, role)
  const isSupervisor = key === 'supervisor'
  const hasTaskBoard = !isSupervisor
  return {
    showInComposer: !isSupervisor,
    showAgentLabel: true,
    showThoughts: true,
    showHeadline: true,
    showSubAgentTrace: isSupervisor,
    showToolCalls: !isSupervisor,
    hideToolNames: hasTaskBoard ? ['task_board', 'task_board:patch'] : [],
    showWorkspacePicker: !isSupervisor,
    showComputerMonitorPicker: key === 'computer',
    showTaskBoardPanel: hasTaskBoard,
    avatar: key
  }
}

function mergeUi(base: ResolvedAgentUi, manifest?: AgentUiConfig, overrides?: Partial<AgentUiConfig>): ResolvedAgentUi {
  const m = manifest ?? {}
  const o = overrides ?? {}
  const pick = <K extends keyof ResolvedAgentUi>(k: K): ResolvedAgentUi[K] => {
    const ov = o[k as keyof AgentUiConfig]
    if (ov !== undefined && ov !== null) return ov as ResolvedAgentUi[K]
    const mv = m[k as keyof AgentUiConfig]
    if (mv !== undefined && mv !== null) return mv as ResolvedAgentUi[K]
    return base[k]
  }
  return {
    showInComposer: pick('showInComposer') as boolean,
    showAgentLabel: pick('showAgentLabel') as boolean,
    showThoughts: pick('showThoughts') as boolean,
    showHeadline: pick('showHeadline') as boolean,
    showSubAgentTrace: pick('showSubAgentTrace') as boolean,
    showToolCalls: pick('showToolCalls') as boolean,
    hideToolNames: (o.hideToolNames ?? m.hideToolNames ?? base.hideToolNames) as string[],
    showWorkspacePicker: pick('showWorkspacePicker') as boolean,
    showComputerMonitorPicker: pick('showComputerMonitorPicker') as boolean,
    showTaskBoardPanel: pick('showTaskBoardPanel') as boolean,
    avatar: (o.avatar ?? m.avatar ?? base.avatar) as string
  }
}

export function resolveAgentUi(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): ResolvedAgentUi {
  if (!agent) {
    return profileDefaults('general', 'default', 'worker')
  }
  const base = profileDefaults(agent.profile, agent.id, agent.role)
  const overrides = settings?.agentUiOverrides?.[agent.id]
  return mergeUi(base, agent.ui, overrides)
}

export function resolveLeadAgentUi(
  settings: ModelSettings,
  agents: AgentDef[]
): ResolvedAgentUi {
  if (settings.agentMode === 'supervisor') {
    const sup = agents.find(a => a.id === 'supervisor' || a.role === 'supervisor')
    return resolveAgentUi(sup, settings)
  }
  const id = settings.leadAgentId?.trim() || 'default'
  const w = agents.find(a => a.id === id) ?? agents.find(a => a.id === 'default')
  return resolveAgentUi(w, settings)
}

/** Show sub-agent timeline when configured or when stream already has delegated steps. */
export function shouldShowSubAgentTrace(
  ui: ResolvedAgentUi,
  agentTrace?: { depth?: number }[],
  settings?: Pick<ModelSettings, 'agentUiOverrides'>,
  agentId?: string
): boolean {
  const id = agentId?.trim()
  if (id && settings?.agentUiOverrides?.[id]?.showSubAgentTrace === false) {
    return false
  }
  if (ui.showSubAgentTrace) return true
  return (agentTrace?.some(a => (a.depth ?? 0) > 0) ?? false)
}
