import { defineStore } from 'pinia'
import { ref } from 'vue'
import { importSkillZip, listSkills, reloadSkillMeta } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
import { useSettingsStore } from './settings'
import { agentsCache, ensureAgentsCatalog } from '../composables/useAgentUi'
import { missingDefaultSystemSkillIds, missingSystemSkillIds } from '../lib/skillEnablement'


export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  const loaded = ref(false)

  // ── init / migration ──

  /** One-time migration: move legacy enabledSkillIds into agentSkillOverrides['general']. */
  function initEnabledFromUserSettings() {
    const user = useSettingsStore().userSettings
    // Legacy field removed from type; migration completed in prior versions
    const legacy = (user as any).enabledSkillIds as string[] | undefined
    if (!legacy || legacy.length === 0) return
    // Only migrate when general doesn't already have an override
    if (user.agentSkillOverrides?.[GENERAL_AGENT_ID]) return
    const next = { ...(user.agentSkillOverrides ?? {}) }
    next[GENERAL_AGENT_ID] = [...legacy]
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
    if (agent) return [...agent.defaultSkillIds]
    // Agents not yet loaded — return empty list rather than fall back to global
    return []
  }

  function enabledIdsForAgent(agentId: string): string[] {
    return agentOverrideIds(agentId) ?? agentDefaultSkillIds(agentId)
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
    await settings.saveUser({
      agentSkillOverrides: {
        ...(settings.userSettings.agentSkillOverrides ?? {}),
        [agentId]: [...new Set(ids)]
      }
    })
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

  // ── system skills ──

  /**
   * Ensure bundled system skills stay enabled after app upgrades.
   * - `general`: all `provenance=system` skills
   * - other agents with a saved override: system skills listed in that agent's
   *   `defaultSkillIds` (e.g. coder must keep `skill-manager`)
   */
  async function ensureSystemSkillsEnabled() {
    const systemIds = skills.value
      .filter(s => s.provenance === 'system')
      .map(s => s.id)
    if (systemIds.length === 0) return
    const systemSet = new Set(systemIds)

    const missingGeneral = missingSystemSkillIds(systemIds, enabledIdsForAgent(GENERAL_AGENT_ID))
    if (missingGeneral.length > 0) {
      console.info(
        '[skills] ensureSystemSkillsEnabled: merged system skills into general',
        missingGeneral
      )
      await setAgentEnabledIds(GENERAL_AGENT_ID, [
        ...enabledIdsForAgent(GENERAL_AGENT_ID),
        ...missingGeneral
      ])
    }

    try {
      await ensureAgentsCatalog()
    } catch (e) {
      console.warn('[skills] ensureSystemSkillsEnabled: agents catalog unavailable', e)
      return
    }

    for (const agent of agentsCache.value) {
      if (agent.id === GENERAL_AGENT_ID) continue
      if (!hasAgentOverride(agent.id)) continue
      const missingDefaults = missingDefaultSystemSkillIds(
        agent.defaultSkillIds ?? [],
        systemSet,
        enabledIdsForAgent(agent.id)
      )
      if (missingDefaults.length === 0) continue
      console.info(
        '[skills] ensureSystemSkillsEnabled: merged default system skills into',
        agent.id,
        missingDefaults
      )
      await setAgentEnabledIds(agent.id, [
        ...enabledIdsForAgent(agent.id),
        ...missingDefaults
      ])
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
