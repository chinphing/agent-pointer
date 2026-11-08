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
