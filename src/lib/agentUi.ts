import type { AgentDef, AgentProfile, AgentUiConfig, ModelSettings } from '../types/chat'

/**
 * Debug mode now only gates the dedicated debug settings entry.
 * Keep display behavior identical to the normal profile unless an explicit
 * agent UI override is already resolved by `mergeUi`.
 */
export function mergeDebugDisplayUi(
  _settings: Pick<ModelSettings, 'debugMenusEnabled' | 'agentUiOverrides'> | undefined,
  _agentId: string,
  ui: ResolvedAgentUi
): ResolvedAgentUi {
  return ui
}
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { t } from '../i18n'
import { agentRoleLabel } from './agentLabels'

/** Built-in general agent id. */
export const GENERAL_AGENT_ID = 'general'

/** Built-in coder agent id. */
export const CODER_AGENT_ID = 'coder'

/** Default skills for the coder lead agent. Keep in sync with coder `defaultSkillIds`. */
export const CODER_DEFAULT_SKILL_IDS = [
  'find-skills',
  'skill-manager',
  'xlsx',
  'pdf',
  'agent-browser',
  'dev-env-setup',
  'docx',
  'pptx'
] as const

export interface ResolvedAgentUi {
  showInComposer: boolean
  showSidecarToolCalls: boolean
  showNonSidecarToolCalls: boolean
  showReasoning: boolean
  showSubAgentTrace: boolean
  showToolCalls: boolean
  showToolCallResults: boolean
  hideToolNames: string[]
  showWorkspacePicker: boolean
  showComputerMonitorPicker: boolean
  showTaskBoardPanel: boolean
  userSelectable: boolean
  composerLabel: string
  avatar: string
}

function profileKey(profile: AgentProfile, id: string): string {
  if (typeof profile === 'string') {
    if (profile === 'computer' || id === 'computer') return 'computer'
    if (profile === 'coder' || id === 'coder') return 'coder'
    if (profile === 'explore' || id === 'explore') return 'explore'
  }
  return 'general'
}

function composerSelectableByProfile(id: string, key: string): boolean {
  return (
    id === 'general' ||
    id === 'coder' ||
    id === 'computer' ||
    key === 'general' ||
    key === 'coder' ||
    key === 'computer'
  )
}

function profileDefaults(profile: AgentProfile, id: string): ResolvedAgentUi {
  const key = profileKey(profile, id)
  return {
    showInComposer: true,
    showSidecarToolCalls: false,
    showNonSidecarToolCalls: true,
    showReasoning: false,
    showSubAgentTrace: false,
    showToolCalls: true,
    showToolCallResults: true,
    hideToolNames: ['task_board_init', 'task_board_patch', 'task_board_replace', 'task_board_finalize', 'task_board_check_deps', 'task_board_prune'],
    showWorkspacePicker: true,
    showComputerMonitorPicker: key === 'computer',
    showTaskBoardPanel: true,
    userSelectable: composerSelectableByProfile(id, key),
    composerLabel: agentRoleLabel(id) || agentRoleLabel(key) || '',
    avatar: key
  }
}

function mergeUi(
  base: ResolvedAgentUi,
  manifest: AgentUiConfig | undefined,
  overrides: Partial<AgentUiConfig> | undefined,
  agentId: string
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
  const labelFallback = agentRoleLabel(agentId) || base.composerLabel
  // Prefer locale-aware role label over AGENT.md Chinese composerLabel.
  const roleLabel = agentRoleLabel(agentId)
  const overrideLabel = (o.composerLabel ?? '').trim()
  const composerLabel =
    overrideLabel ||
    (roleLabel && roleLabel !== agentId.trim() ? roleLabel : '') ||
    ((m.composerLabel ?? base.composerLabel) as string).trim() ||
    labelFallback
  return {
    showInComposer: pick('showInComposer') as boolean,
    showSidecarToolCalls: pick('showSidecarToolCalls') as boolean,
    showNonSidecarToolCalls: pick('showNonSidecarToolCalls') as boolean,
    showReasoning: pick('showReasoning') as boolean,
    showSubAgentTrace: pick('showSubAgentTrace') as boolean,
    showToolCalls: pick('showToolCalls') as boolean,
    showToolCallResults: pick('showToolCallResults') as boolean,
    hideToolNames: (o.hideToolNames ?? m.hideToolNames ?? base.hideToolNames) as string[],
    showWorkspacePicker: pick('showWorkspacePicker') as boolean,
    showComputerMonitorPicker: pick('showComputerMonitorPicker') as boolean,
    showTaskBoardPanel: pick('showTaskBoardPanel') as boolean,
    userSelectable: pick('userSelectable') as boolean,
    composerLabel,
    avatar: (o.avatar ?? m.avatar ?? base.avatar) as string
  }
}

export function composerAgentLabel(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): string {
  if (!agent) return t('agents.general')
  // Built-in roles: prefer vue-i18n so AGENT.md Chinese composerLabel
  // (e.g. 通用助手) does not win over the active UI locale.
  const roleLabel = agentRoleLabel(agent.id)
  if (roleLabel && roleLabel !== agent.id.trim()) {
    const override = settings?.agentUiOverrides?.[agent.id]?.composerLabel?.trim()
    if (override) return override
    return roleLabel
  }
  const ui = resolveAgentUi(agent, settings)
  return ui.composerLabel.trim() || roleLabel || t('agents.general')
}

/** User-visible label for an agent id (cron list, traces, etc.). */
export function composerAgentLabelById(
  agentId: string | null | undefined,
  agents?: AgentDef[],
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): string {
  const id = agentId?.trim() || DEFAULT_LEAD_AGENT_ID
  const agent = agents?.find(a => a.id === id)
  if (agent) return composerAgentLabel(agent, settings)
  return agentRoleLabel(id) || id
}

/** Resolve user-visible label for a sub-agent trace row (handles legacy English slug in `trace.name`). */
export function traceAgentLabel(
  trace: { id: string; name: string; agentId?: string },
  agents: AgentDef[],
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): string {
  const agentId = (trace.agentId?.trim() || (trace.id.includes(':')
    ? trace.id.slice(trace.id.lastIndexOf(':') + 1)
    : '')).trim()
  const agent = agentId ? agents.find(a => a.id === agentId) : undefined
  if (agent) return composerAgentLabel(agent, settings)
  const stored = trace.name.trim()
  if (stored && !stored.includes('-')) return stored
  return agentRoleLabel(agentId) || stored || t('chat.subtask')
}

export function resolveAgentUi(
  agent: AgentDef | undefined,
  settings?: Pick<ModelSettings, 'agentUiOverrides'>
): ResolvedAgentUi {
  if (!agent) {
    return profileDefaults('general', 'general')
  }
  const base = profileDefaults(agent.profile, agent.id)
  const overrides = settings?.agentUiOverrides?.[agent.id]
  return mergeDebugDisplayUi(settings, agent.id, mergeUi(base, agent.ui, overrides, agent.id))
}

function leadAgentProfile(id: string): AgentProfile {
  if (id === 'computer') return 'computer'
  if (id === 'coder') return 'coder'
  if (id === 'explore') return 'explore'
  return 'general'
}

export function resolveLeadAgentUi(
  settings: ModelSettings,
  agents: AgentDef[]
): ResolvedAgentUi {
  const id = settings.leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  const w = agents.find(a => a.id === id) ?? agents.find(a => a.id === GENERAL_AGENT_ID)
  if (w) return resolveAgentUi(w, settings)
  return mergeDebugDisplayUi(
    settings,
    id,
    profileDefaults(leadAgentProfile(id), id)
  )
}

/** Show sub-agent frame when configured, or when the stream already has delegated steps. */
export function shouldShowSubAgentTrace(
  ui: ResolvedAgentUi,
  agentTrace?: { depth?: number; status?: string }[],
  settings?: Pick<ModelSettings, 'agentUiOverrides'>,
  agentId?: string
): boolean {
  const id = agentId?.trim()
  if (id && settings?.agentUiOverrides?.[id]?.showSubAgentTrace === false) {
    return false
  }
  if (ui.showSubAgentTrace) return true
  const hasDelegated = agentTrace?.some(t => (t.depth ?? 0) > 0) ?? false
  return hasDelegated
}

/** UI flags for content inside a sub-agent frame (always show tools/reasoning). */
export function uiForSubAgentFrame(
  trace: { id: string; name: string; role?: string; agentId?: string },
  settings: Pick<ModelSettings, 'agentUiOverrides' | 'agentMode' | 'leadAgentId'>,
  agents: import('../types/chat').AgentDef[],
  fallbackUi: ResolvedAgentUi
): ResolvedAgentUi {
  const agentId = (trace.agentId?.trim() || (trace.id.includes(':')
    ? trace.id.slice(trace.id.lastIndexOf(':') + 1)
    : '')).trim()
  const agent = agentId ? agents.find(a => a.id === agentId) : undefined
  const base = agent ? resolveAgentUi(agent, settings) : fallbackUi
  return {
    ...base,
    showToolCalls: true,
    showSubAgentTrace: true
  }
}
