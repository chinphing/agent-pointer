/**
 * 角色标签的增删辅助。
 *
 * 标签点一下就移除，但**只渲染已有角色**，所以没有「加回来」的入口——
 * 员工误点清空角色后，界面上再也加不回去。这里提供可添加的角色列表。
 */

export interface RoleLike {
  key: string
  name?: string
}

/** 还没分配给该用户的角色（用于「+ 角色」下拉）。 */
export function addableRoles<T extends RoleLike>(
  all: readonly T[],
  assigned: readonly string[]
): T[] {
  const has = new Set(assigned)
  return all.filter((r) => !has.has(r.key))
}

/**
 * 角色的显示名。
 *
 * 内置角色（super_admin / admin / viewer / member）一律走 i18n 键，
 * 后端返回的中文名不再直接上屏 —— 否则切英文时角色名还是中文。
 * 自定义角色用管理员起的名字，没有名字才退回 key。
 */
const BUILTIN_ROLE_KEYS = ['super_admin', 'admin', 'viewer', 'member']

export function isBuiltinRole(key: string): boolean {
  return BUILTIN_ROLE_KEYS.includes(key)
}

export function roleLabel(
  key: string,
  fallbackName: string | undefined,
  translate: (k: string) => string
): string {
  if (isBuiltinRole(key)) return translate(`roles.${key}`)
  return (fallbackName || '').trim() || key
}
