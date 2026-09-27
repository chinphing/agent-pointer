/** Agent role id → 用户可见中文名。未知 id 原样显示。 */
const AGENT_ROLE_LABELS: Record<string, string> = {
  general: '通用助手',
  coder: '氛围编程',
  computer: '电脑操控'
}

export function agentRoleLabel(roleId: string | null | undefined): string {
  const id = (roleId ?? '').trim()
  if (!id) return ''
  return AGENT_ROLE_LABELS[id] ?? id
}
