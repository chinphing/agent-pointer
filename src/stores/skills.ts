import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { importSkillZip, listSkills, reloadSkillMeta } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'
import { DEFAULT_ENABLED_SKILL_IDS } from '../types/chat'
import { useSettingsStore } from './settings'


export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  /** Global enabled skill ids; persisted via user_settings.json. */
  const enabledIds = ref<string[]>([])
  const loaded = ref(false)

  const enabledSkills = computed(() =>
    skills.value.filter(s => enabledIds.value.includes(s.id))
  )

  function initEnabledFromUserSettings() {
    const user = useSettingsStore().userSettings
    enabledIds.value =
      user.enabledSkillIds !== undefined
        ? [...user.enabledSkillIds]
        : [...DEFAULT_ENABLED_SKILL_IDS]
  }

  function agentOverrideIds(agentId: string): string[] | undefined {
    const override = useSettingsStore().userSettings.agentSkillOverrides?.[agentId]
    return override ? [...override] : undefined
  }

  function enabledIdsForAgent(agentId: string): string[] {
    return agentOverrideIds(agentId) ?? [...enabledIds.value]
  }

  function hasAgentOverride(agentId: string): boolean {
    return Object.prototype.hasOwnProperty.call(
      useSettingsStore().userSettings.agentSkillOverrides ?? {},
      agentId
    )
  }

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

  /** Ensure every system-bundled skill is enabled (e.g. after app adds new built-ins). */
  async function ensureSystemSkillsEnabled() {
    const systemIds = skills.value
      .filter(s => s.provenance === 'system')
      .map(s => s.id)
    if (systemIds.length === 0) return
    const merged = [...enabledIds.value]
    let changed = false
    for (const id of systemIds) {
      if (!merged.includes(id)) {
        merged.push(id)
        changed = true
      }
    }
    if (!changed) return
    enabledIds.value = merged
    await persistEnabledIds()
  }

  async function persistEnabledIds() {
    await useSettingsStore().saveUser({ enabledSkillIds: [...enabledIds.value] })
  }

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
      const merged = [...enabledIds.value]
      let changed = false
      for (const s of result.imported) {
        if (!merged.includes(s.id)) {
          merged.push(s.id)
          changed = true
        }
      }
      if (changed) {
        enabledIds.value = merged
        await persistEnabledIds()
      }
    }
    return result
  }

  async function toggle(id: string) {
    const i = enabledIds.value.indexOf(id)
    if (i >= 0) enabledIds.value.splice(i, 1)
    else enabledIds.value.push(id)
    await persistEnabledIds()
  }

  function isEnabled(id: string) {
    return enabledIds.value.includes(id)
  }

  async function setEnabledIds(ids: string[]) {
    enabledIds.value = [...ids]
    await persistEnabledIds()
  }

  return {
    skills,
    enabledIds,
    enabledSkills,
    loaded,
    load,
    importZip,
    toggle,
    isEnabled,
    setEnabledIds,
    agentOverrideIds,
    enabledIdsForAgent,
    hasAgentOverride,
    setAgentEnabledIds,
    toggleForAgent,
    resetAgentOverride,
    initEnabledFromUserSettings,
    ensureSystemSkillsEnabled,
    persistEnabledIds
  }

})
