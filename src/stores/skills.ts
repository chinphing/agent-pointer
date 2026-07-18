import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { importSkillZip, listSkills, reloadSkillMeta } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'
import { DEFAULT_ENABLED_SKILL_IDS } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
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
    const generalOverrides = user.agentSkillOverrides?.[GENERAL_AGENT_ID]
    if (generalOverrides) {
      enabledIds.value = [...generalOverrides]
    } else if (user.enabledSkillIds !== undefined) {
      enabledIds.value = [...user.enabledSkillIds]
    } else {
      enabledIds.value = [...DEFAULT_ENABLED_SKILL_IDS]
    }
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
    const settings = useSettingsStore()
    const overrides = { ...settings.userSettings.agentSkillOverrides }
    overrides[GENERAL_AGENT_ID] = [...enabledIds.value]
    await settings.saveUser({
      enabledSkillIds: [...enabledIds.value],
      agentSkillOverrides: overrides
    })
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
    initEnabledFromUserSettings,
    ensureSystemSkillsEnabled,
    persistEnabledIds
  }

})
