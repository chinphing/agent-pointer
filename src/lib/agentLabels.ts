import { t } from '../i18n'

/** Agent role id → user-visible label. Unknown ids are shown as-is. */
const AGENT_ROLE_LABEL_KEYS: Record<string, string> = {
  general: 'agent.role.general',
  coder: 'agent.role.coder',
  computer: 'agent.role.computer'
}

export function agentRoleLabel(roleId: string | null | undefined): string {
  const id = (roleId ?? '').trim()
  if (!id) return ''
  const key = AGENT_ROLE_LABEL_KEYS[id]
  return key ? t(key) : id
}
