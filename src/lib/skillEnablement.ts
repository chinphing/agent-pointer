/** System skill ids missing from an agent's current enabled list. */
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
