/**
 * 企业 `default_on` 技能的合并规则（与 Rust 侧 `merge_org_default_on` 同口径）：
 * 已在列表里的不重复加；员工显式关过（opt-out）的不加。
 */
export function mergeOrgDefaultOn(
  ids: readonly string[],
  defaultOnIds: readonly string[],
  optedOutIds: readonly string[]
): string[] {
  const out = [...ids]
  const optedOut = new Set(optedOutIds)
  for (const id of defaultOnIds) {
    if (optedOut.has(id) || out.includes(id)) continue
    out.push(id)
  }
  return out
}

export function missingSystemSkillIds(
  systemIds: readonly string[],
  enabledIds: readonly string[]
): string[] {
  const enabled = new Set(enabledIds)
  return systemIds.filter(id => !enabled.has(id))
}

/**
 * Default skill ids that are system-bundled but absent from the agent's
 * saved override (e.g. coder missing skill-manager after an upgrade).
 */
export function missingDefaultSystemSkillIds(
  defaultSkillIds: readonly string[],
  systemIds: ReadonlySet<string>,
  enabledIds: readonly string[]
): string[] {
  const enabled = new Set(enabledIds)
  return defaultSkillIds.filter(id => systemIds.has(id) && !enabled.has(id))
}
