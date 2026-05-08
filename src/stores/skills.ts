import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { listSkills } from '../lib/api'
import type { SkillDef } from '../types/chat'

export const useSkillsStore = defineStore('skills', () => {
  const skills = ref<SkillDef[]>([])
  const enabledIds = ref<string[]>([])
  const loaded = ref(false)

  const enabledSkills = computed(() =>
    skills.value.filter(s => enabledIds.value.includes(s.id))
  )

  async function load() {
    skills.value = await listSkills()
    loaded.value = true
  }

  function toggle(id: string) {
    const i = enabledIds.value.indexOf(id)
    if (i >= 0) enabledIds.value.splice(i, 1)
    else enabledIds.value.push(id)
  }

  function isEnabled(id: string) {
    return enabledIds.value.includes(id)
  }

  return { skills, enabledIds, enabledSkills, loaded, load, toggle, isEnabled }
})
