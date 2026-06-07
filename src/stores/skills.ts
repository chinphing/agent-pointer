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

  async function persistEnabledIds() {
    await useSettingsStore().saveUser({ enabledSkillIds: [...enabledIds.value] })
  }

  async function load(options?: { rescan?: boolean }) {
    try {
      skills.value = options?.rescan ? await reloadSkillMeta() : await listSkills()
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
    persistEnabledIds
  }

})
