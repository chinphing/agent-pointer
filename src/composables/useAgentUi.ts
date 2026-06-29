import { ref, computed, onMounted, type ComputedRef } from 'vue'
import { listAgents } from '../lib/api'
import { resolveAgentUi, resolveLeadAgentUi, type ResolvedAgentUi } from '../lib/agentUi'
import type { AgentDef } from '../types/chat'
import { useSettingsStore } from '../stores/settings'

const agentsCache = ref<AgentDef[]>([])
let loaded = false
let loadPromise: Promise<AgentDef[]> | null = null

/** Single shared fetch; concurrent callers await the same in-flight request. */
export function ensureAgentsCatalog(): Promise<AgentDef[]> {
  if (loaded) return Promise.resolve(agentsCache.value)
  if (!loadPromise) {
    loadPromise = listAgents()
      .then(list => {
        agentsCache.value = list
        loaded = true
        return list
      })
      .catch(e => {
        loadPromise = null
        throw e
      })
  }
  return loadPromise
}

export function useAgentsCatalog() {
  onMounted(() => {
    void ensureAgentsCatalog().catch(e => console.warn('[agents] list failed', e))
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
