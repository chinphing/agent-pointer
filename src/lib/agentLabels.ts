import { t, te } from '../i18n'

/** Agent role id → i18n key under `agents.*`. Unknown ids shown as-is. */
const AGENT_ROLE_KEYS: Record<string, string> = {
  general: 'agents.general',
  coder: 'agents.coder',
  computer: 'agents.computer',
  explore: 'agents.explore',
  general_worker: 'agents.generalWorker',
  'general-worker': 'agents.generalWorker',
  analyst: 'agents.analyst',
  supervisor: 'agents.supervisor',
  team: 'agents.teamMode'
}

export function agentRoleLabel(roleId: string | null | undefined): string {
  const id = (roleId ?? '').trim()
  if (!id) return ''
  const key = AGENT_ROLE_KEYS[id]
  if (key && te(key)) return t(key)
  return id
}
