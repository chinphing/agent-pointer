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
    if (agent) return [...agent.defaultSkillIds]
    return []
  }

  /** UI effective list: override ?? defaults (runtime also merges missing bundled defaults). */
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
