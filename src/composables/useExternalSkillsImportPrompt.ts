import { ref } from 'vue'
import {
  dismissExternalSkillsPrompt,
  importExternalSkills,
  probeExternalSkills
} from '../lib/api'
import type { ExternalSkillSource } from '../types/chat'

export function useExternalSkillsImportPrompt() {
  const open = ref(false)
  const sources = ref<ExternalSkillSource[]>([])
  const totalSkills = ref(0)
  const importing = ref(false)

  async function checkOnBoot() {
    try {
      const probe = await probeExternalSkills()
      if (!probe.shouldPrompt || probe.sources.length === 0) return
      sources.value = probe.sources
      totalSkills.value = probe.totalSkills
      open.value = true
    } catch (e) {
      console.warn('[external-skills-probe]', e)
    }
  }

  async function dismiss() {
    open.value = false
    try {
      await dismissExternalSkillsPrompt()
    } catch (e) {
      console.warn('[external-skills-dismiss]', e)
    }
  }

  async function importSelected(sourceIds: string[], reloadSkills: () => Promise<void>) {
    if (sourceIds.length === 0) return
    importing.value = true
    try {
      await importExternalSkills(sourceIds)
      await reloadSkills()
      open.value = false
    } catch (e) {
      console.error('[external-skills-import]', e)
      throw e
    } finally {
      importing.value = false
    }
  }

  return {
    open,
    sources,
    totalSkills,
    importing,
    checkOnBoot,
    dismiss,
    importSelected
  }
}
