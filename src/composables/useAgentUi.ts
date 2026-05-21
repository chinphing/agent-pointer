import { ref, computed, onMounted, type ComputedRef } from 'vue'
import { listAgents } from '../lib/api'
import { resolveAgentUi, resolveLeadAgentUi, type ResolvedAgentUi } from '../lib/agentUi'
import type { AgentDef } from '../types/chat'
import { useSettingsStore } from '../stores/settings'

const agentsCache = ref<AgentDef[]>([])
let loaded = false

export function useAgentsCatalog() {
  onMounted(() => {
    if (loaded) return
    void listAgents()
      .then(list => {
        agentsCache.value = list
        loaded = true
      })
      .catch(e => console.warn('[agents] list failed', e))
  })
  return agentsCache
}

export function useLeadAgentUi(): {
  leadUi: ComputedRef<ResolvedAgentUi>
  agents: typeof agentsCache
} {
  const settings = useSettingsStore()
  const agents = useAgentsCatalog()
  const leadUi = computed(() => resolveLeadAgentUi(settings.settings, agents.value))
  return { leadUi, agents }
}

export function uiForMessageAgent(
  agentId: string | undefined,
  agentName: string | undefined,
  settings: ReturnType<typeof useSettingsStore>['settings'],
  agents: AgentDef[]
): ResolvedAgentUi {
  const id = agentId?.trim()
  if (id) {
    const a = agents.find(x => x.id === id)
    if (a) return resolveAgentUi(a, settings)
  }
  return resolveLeadAgentUi(settings, agents)
}
