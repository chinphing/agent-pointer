import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { importSkillZip, listSkills, reloadSkillMeta } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'


export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  const enabledIds = ref<string[]>([])
  const loaded = ref(false)

  const enabledSkills = computed(() =>
    skills.value.filter(s => enabledIds.value.includes(s.id))
  )

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

  function toggle(id: string) {

    const i = enabledIds.value.indexOf(id)
    if (i >= 0) enabledIds.value.splice(i, 1)
    else enabledIds.value.push(id)
  }

  function isEnabled(id: string) {
    return enabledIds.value.includes(id)
  }

  function setEnabledIds(ids: string[]) {
    enabledIds.value = [...ids]
  }

  return { skills, enabledIds, enabledSkills, loaded, load, importZip, toggle, isEnabled, setEnabledIds }

})
