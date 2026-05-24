import type { AgentDef, AgentProfile, AgentUiConfig, ModelSettings } from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'

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
  userSelectable: boolean
  composerLabel: string
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

const COMPOSER_LABELS: Record<string, string> = {
  default: '综合对话',
  coder: '小白编程',
  computer: '电脑操控',
  supervisor: '团队模式',
  explore: '代码探索'
}

function composerSelectableByProfile(id: string, key: string, isSupervisor: boolean): boolean {
  if (isSupervisor) return false
  return id === 'default' || id === 'coder' || id === 'computer' || key === 'default' || key === 'coder' || key === 'computer'
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
    showWorkspacePicker: key === 'coder',
    showComputerMonitorPicker: key === 'computer',
    showTaskBoardPanel: hasTaskBoard,
    userSelectable: composerSelectableByProfile(id, key, isSupervisor),
    composerLabel: COMPOSER_LABELS[key] ?? COMPOSER_LABELS[id] ?? '',
    avatar: key
  }
}

function mergeUi(
  base: ResolvedAgentUi,
  manifest: AgentUiConfig | undefined,
  overrides: Partial<AgentUiConfig> | undefined,
  agentName: string
): ResolvedAgentUi {
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
    userSelectable: pick('userSelectable') as boolean,
    composerLabel: ((o.composerLabel ?? m.composerLabel ?? base.composerLabel) as string).trim() || agentName,
    avatar: (o.avatar ?? m.avatar ?? base.avatar) as string
  }
}

export function composerAgentLabel(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): string {
  if (!agent) return '综合对话'
  const ui = resolveAgentUi(agent, settings)
  return ui.composerLabel.trim() || agent.name
}

export function resolveAgentUi(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides' | 'computerShowMonitorPicker'>
): ResolvedAgentUi {
  if (!agent) {
    return profileDefaults('general', 'default', 'worker')
  }
  const base = profileDefaults(agent.profile, agent.id, agent.role)
  const overrides = settings?.agentUiOverrides?.[agent.id]
  const merged = mergeUi(base, agent.ui, overrides, agent.name)
  const monitorOverride = settings?.agentUiOverrides?.[agent.id]?.showComputerMonitorPicker
  if (
    agent.id === 'computer'
    && monitorOverride === undefined
    && settings?.computerShowMonitorPicker !== undefined
  ) {
    return { ...merged, showComputerMonitorPicker: settings.computerShowMonitorPicker }
  }
  return merged
}

function leadAgentProfile(id: string): AgentProfile {
  if (id === 'computer') return 'computer'
  if (id === 'coder') return 'coder'
  if (id === 'explore') return 'explore'
  if (id === 'supervisor') return 'supervisor'
  return 'general'
}

export function resolveLeadAgentUi(
  settings: ModelSettings,
  agents: AgentDef[]
): ResolvedAgentUi {
  if (settings.agentMode === 'supervisor') {
    const sup = agents.find(a => a.id === 'supervisor' || a.role === 'supervisor')
    if (sup) return resolveAgentUi(sup, settings)
    return profileDefaults('supervisor', 'supervisor', 'supervisor')
  }
  const id = settings.leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  const w = agents.find(a => a.id === id) ?? agents.find(a => a.id === DEFAULT_LEAD_AGENT_ID)
  if (w) return resolveAgentUi(w, settings)
  return profileDefaults(leadAgentProfile(id), id, 'worker')
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
  return false
}
