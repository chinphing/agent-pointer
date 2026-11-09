import { defineStore } from 'pinia'
import { ref } from 'vue'
import { importSkillZip, listSkills, reloadSkillMeta } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
import { useSettingsStore } from './settings'
import { agentsCache, ensureAgentsCatalog } from '../composables/useAgentUi'
import { missingDefaultSystemSkillIds, missingSystemSkillIds, mergeOrgDefaultOn } from '../lib/skillEnablement'

/** `agentSkillOverrides` 里记录「员工显式关掉的企业技能」的保留键（与 Rust 侧 ORG_OPT_OUT_KEY 一致）。 */
const ORG_OPT_OUT_KEY = '_orgOptOut'

export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  const loaded = ref(false)

  // ── init / migration ──

  /** One-time migration: move legacy enabledSkillIds into agentSkillOverrides['general']. */
  function initEnabledFromUserSettings() {
    const user = useSettingsStore().userSettings
    const legacy = user.enabledSkillIds
    if (!legacy || legacy.length === 0) return
    if (user.agentSkillOverrides?.[GENERAL_AGENT_ID]) return
    const next = { ...(user.agentSkillOverrides ?? {}) }
    next[GENERAL_AGENT_ID] = [...legacy]
    console.info(
      '[skills] initEnabledFromUserSettings: migrated legacy enabledSkillIds → general override',
      legacy.length
    )
    void useSettingsStore().saveUser({ agentSkillOverrides: next })
  }

  // ── helpers ──

  function agentOverrideIds(agentId: string): string[] | undefined {
    const override = useSettingsStore().userSettings.agentSkillOverrides?.[agentId]
    return override ? [...override] : undefined
  }

  /** Resolve the agent's default skill ids from its definition (from AGENT.md). */
  function agentDefaultSkillIds(agentId: string): string[] {
    const agent = agentsCache.value.find(a => a.id === agentId)
    return agent ? [...agent.defaultSkillIds] : []
  }

  /** UI effective list: override ?? defaults，再合并企业 `default_on`（员工显式关过的除外）。 */
  function enabledIdsForAgent(agentId: string): string[] {
    const ids = agentOverrideIds(agentId) ?? agentDefaultSkillIds(agentId)
    const defaultOn = skills.value.filter(s => s.defaultEnabled).map(s => s.id)
    return mergeOrgDefaultOn(ids, defaultOn, orgOptOutIds())
  }

  /** 员工显式关掉的企业技能（存在保留键里，与运行时同一口径）。 */
  function orgOptOutIds(): string[] {
    return useSettingsStore().userSettings.agentSkillOverrides?.[ORG_OPT_OUT_KEY] ?? []
  }

  function hasAgentOverride(agentId: string): boolean {
    return Object.prototype.hasOwnProperty.call(
      useSettingsStore().userSettings.agentSkillOverrides ?? {},
      agentId
    )
  }

  // ── mutation ──

  async function setAgentEnabledIds(agentId: string, ids: string[]) {
    const settings = useSettingsStore()
    const overrides = { ...(settings.userSettings.agentSkillOverrides ?? {}) }
    const next = [...new Set(ids)]
    overrides[agentId] = next

    // 企业 `default_on` 的技能被关掉时要记进 opt-out，否则下次合并又给开回来。
    const optOut = new Set(orgOptOutIds())
    for (const skill of skills.value) {
      if (!skill.defaultEnabled) continue
      if (next.includes(skill.id)) optOut.delete(skill.id)
      else optOut.add(skill.id)
    }
    overrides[ORG_OPT_OUT_KEY] = [...optOut]

    await settings.saveUser({ agentSkillOverrides: overrides })
  }

  async function toggleForAgent(agentId: string, skillId: string) {
    const ids = enabledIdsForAgent(agentId)
    const index = ids.indexOf(skillId)
    if (index >= 0) ids.splice(index, 1)
    else ids.push(skillId)
    await setAgentEnabledIds(agentId, ids)
  }

  async function resetAgentOverride(agentId: string) {
    const settings = useSettingsStore()
    const overrides = { ...(settings.userSettings.agentSkillOverrides ?? {}) }
    delete overrides[agentId]
    await settings.saveUser({ agentSkillOverrides: overrides })
  }

  /**
   * Persist missing bundled defaults into agentSkillOverrides so the picker matches
   * runtime resolve (`override ?? defaults` + merge bundled defaults).
   */
  async function ensureSystemSkillsEnabled() {
    const systemIds = skills.value
      .filter(s => s.provenance === 'system')
      .map(s => s.id)
    if (systemIds.length === 0) return
    const systemSet = new Set(systemIds)
    const settings = useSettingsStore()
    const overrides = { ...(settings.userSettings.agentSkillOverrides ?? {}) }
    let changed = false

    try {
      await ensureAgentsCatalog()
    } catch (e) {
      console.warn('[skills] ensureSystemSkillsEnabled: agents catalog unavailable', e)
      return
    }

    for (const agent of agentsCache.value) {
      const defaults = agent.defaultSkillIds ?? []
      if (defaults.length === 0) continue
      if (!hasAgentOverride(agent.id) && agent.id !== GENERAL_AGENT_ID) continue
      const current = enabledIdsForAgent(agent.id)
      const missing =
        agent.id === GENERAL_AGENT_ID
          ? missingSystemSkillIds(systemIds, current)
          : missingDefaultSystemSkillIds(defaults, systemSet, current)
      if (missing.length === 0) continue
      console.info(
        '[skills] ensureSystemSkillsEnabled: merged default system skills into',
        agent.id,
        missing
      )
      overrides[agent.id] = [...current, ...missing]
      changed = true
    }

    if (changed) {
      await settings.saveUser({ agentSkillOverrides: overrides })
    }
  }

  // ── lifecycle ──

  async function load(options?: { rescan?: boolean }) {
    try {
      skills.value = options?.rescan ? await reloadSkillMeta() : await listSkills()
      await ensureSystemSkillsEnabled()
    } catch (e) {
      console.error('skills.load failed', e)
      skills.value = []
    } finally {
      loaded.value = true
    }
  }

  async function importZip(file: File): Promise<SkillImportResult> {
    const result = await importSkillZip(file)
    await load({ rescan: true })
    if (result.imported.length > 0) {
      const current = enabledIdsForAgent(GENERAL_AGENT_ID)
      let changed = false
      for (const s of result.imported) {
        if (!current.includes(s.id)) {
          current.push(s.id)
          changed = true
        }
      }
      if (changed) {
        await setAgentEnabledIds(GENERAL_AGENT_ID, current)
      }
    }
    return result
  }

  return {
    skills,
    loaded,
    load,
    importZip,
    agentOverrideIds,
    enabledIdsForAgent,
    hasAgentOverride,
    setAgentEnabledIds,
    toggleForAgent,
    resetAgentOverride,
    initEnabledFromUserSettings,
    ensureSystemSkillsEnabled
  }
})
