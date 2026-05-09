import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getSettings, updateSettings, setApiKey, clearApiKey, testConnection } from '../lib/api'
import type { ModelSettings, ProviderConfig } from '../types/chat'

const defaultProviders: ProviderConfig[] = [
  {
    id: 'qwen',
    name: '阿里云千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: '',
    models: ['qwen3.5-plus', 'qwen3.6-plus', 'qwen3.5-flash', 'qwen3.5-27b']
  }
]

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<ModelSettings>({
    providers: defaultProviders,
    activeProviderId: 'qwen',
    model: 'qwen3.5-plus',
    temperature: 0.7,
    maxTokens: 2048,
    hasKey: false,
    toolApprovalMode: 'auto',
    agentMode: 'single',
    workspaceRoot: '',
    leadAgentId: '',
    contextCompressionEnabled: true,
    contextBudgetChars: 120_000,
    contextKeepRecentUserTurns: 6,
    contextSummaryMaxTokens: 1024,
    maxToolRounds: 100
  })
  const loading = ref(false)
  const testing = ref(false)
  const testResult = ref<{ ok: boolean; latencyMs: number; message: string } | null>(null)

  const activeProvider = computed(() =>
    settings.value.providers.find(p => p.id === settings.value.activeProviderId) || settings.value.providers[0]
  )

  const activeBaseUrl = computed(() => activeProvider.value.baseUrl)
  const activeModelList = computed(() => activeProvider.value.models)

  async function load() {
    loading.value = true
    const s = await getSettings().catch(() => null)
    if (s) {
      if (s.providers && s.providers.length > 0) {
        settings.value = {
          ...s,
          workspaceRoot: s.workspaceRoot ?? '',
          leadAgentId: s.leadAgentId ?? '',
          contextCompressionEnabled: s.contextCompressionEnabled ?? true,
          contextBudgetChars: s.contextBudgetChars ?? 120_000,
          contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 6,
          contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
          maxToolRounds: s.maxToolRounds ?? 100
        }
      } else {
        settings.value = {
          ...settings.value,
          ...s,
          providers: s.providers || defaultProviders,
          activeProviderId: s.activeProviderId || 'qwen',
          workspaceRoot: s.workspaceRoot ?? '',
          leadAgentId: s.leadAgentId ?? '',
          contextCompressionEnabled: s.contextCompressionEnabled ?? true,
          contextBudgetChars: s.contextBudgetChars ?? 120_000,
          contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 6,
          contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
          maxToolRounds: s.maxToolRounds ?? 100
        }
        if (s.model && !settings.value.providers.find(p => p.id === settings.value.activeProviderId)?.models.includes(s.model)) {
          settings.value.model = settings.value.providers.find(p => p.id === settings.value.activeProviderId)?.models[0] || s.model
        }
      }
    }
    loading.value = false
  }

  async function save(patch: Partial<ModelSettings>) {
    const merged: ModelSettings = { ...settings.value, ...patch }
    const updated = await updateSettings(merged)
    settings.value = updated
  }

  function setActiveProvider(id: string) {
    const provider = settings.value.providers.find(p => p.id === id)
    if (provider) {
      settings.value.activeProviderId = id
      if (!provider.models.includes(settings.value.model)) {
        settings.value.model = provider.models[0]
      }
    }
  }

  function addProvider(provider: ProviderConfig) {
    settings.value.providers.push(provider)
  }

  function updateProvider(id: string, patch: Partial<ProviderConfig>) {
    const provider = settings.value.providers.find(p => p.id === id)
    if (provider) {
      Object.assign(provider, patch)
    }
  }

  function removeProvider(id: string) {
    settings.value.providers = settings.value.providers.filter(p => p.id !== id)
    if (settings.value.activeProviderId === id && settings.value.providers.length > 0) {
      settings.value.activeProviderId = settings.value.providers[0].id
    }
  }

  async function saveProviderKey(providerId: string, key: string) {
    const provider = settings.value.providers.find(p => p.id === providerId)
    if (provider) {
      provider.apiKey = key
    }
    if (providerId === settings.value.activeProviderId) {
      await setApiKey(key)
      settings.value.hasKey = !!key
    }
  }

  async function saveKey(key: string) {
    await saveProviderKey(settings.value.activeProviderId, key)
  }

  async function removeKey() {
    await saveProviderKey(settings.value.activeProviderId, '')
    await clearApiKey()
    settings.value.hasKey = false
  }

  async function runTest() {
    testing.value = true
    testResult.value = null
    const r = await testConnection().catch(e => ({ ok: false, latencyMs: 0, message: String(e) }))
    testResult.value = r
    testing.value = false
  }

  return {
    settings, loading, testing, testResult, activeProvider, activeBaseUrl, activeModelList,
    load, save, setActiveProvider, addProvider, updateProvider, removeProvider,
    saveKey, removeKey, runTest
  }
})
