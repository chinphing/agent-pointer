import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { importSkillZip, listSkills } from '../lib/api'
import type { SkillDef, SkillImportResult } from '../types/chat'


export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  const enabledIds = ref<string[]>([])
  const loaded = ref(false)

  const enabledSkills = computed(() =>
    skills.value.filter(s => enabledIds.value.includes(s.id))
  )

  async function load() {
    try {
      skills.value = await listSkills()
    } catch (e) {
      console.error('listSkills failed', e)
      skills.value = []
    } finally {
      loaded.value = true
    }
  }

  async function importZip(file: File): Promise<SkillImportResult> {
    const result = await importSkillZip(file)
    await load()
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

  return { skills, enabledIds, enabledSkills, loaded, load, importZip, toggle, isEnabled }

})
