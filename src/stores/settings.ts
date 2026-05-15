import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getSettings, updateSettings, setApiKey, clearApiKey, testConnection } from '../lib/api'
import type { AgentModelRef, ModelSettings, ProviderConfig } from '../types/chat'

const defaultProviders: ProviderConfig[] = [
  {
    id: 'qwen',
    name: '千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: '',
    models: ['qwen3.5-plus', 'qwen3.6-plus', 'qwen3.5-flash', 'qwen3.5-27b'],
    reasoningInMessages: true,
    modelConfigs: {}
  },
  {
    id: 'deepseek',
    name: '深度求索',
    baseUrl: 'https://api.deepseek.com/v1',
    apiKey: '',
    models: ['deepseek-v4-flash', 'deepseek-v4-pro'],
    reasoningInMessages: true,
    modelConfigs: {}
  }
]

/** Normalize provider entries from API; merge legacy root `reasoningInMessages` when per-provider value is absent. */
function normalizeProvider(p: ProviderConfig, legacyReasoning?: boolean): ProviderConfig {
  return {
    ...p,
    modelConfigs: p.modelConfigs ? { ...p.modelConfigs } : {},
    reasoningInMessages:
      p.reasoningInMessages !== undefined
        ? p.reasoningInMessages
        : legacyReasoning !== undefined
          ? legacyReasoning
          : undefined
  }
}

function normalizeProviders(
  list: ProviderConfig[] | undefined,
  legacyReasoning?: boolean
): ProviderConfig[] {
  const raw = list?.length ? list : defaultProviders
  return raw.map(p => normalizeProvider(p, legacyReasoning))
}

function normalizeAgentDefaultModels(
  raw: Record<string, AgentModelRef> | Record<string, unknown> | undefined,
  activeProviderId: string
): Record<string, AgentModelRef> {
  const fid = (activeProviderId || 'qwen').trim() || 'qwen'
  const out: Record<string, AgentModelRef> = {}
  if (!raw || typeof raw !== 'object') return out
  for (const [k, v] of Object.entries(raw)) {
    if (v && typeof v === 'object' && !Array.isArray(v) && 'model' in (v as object)) {
      const o = v as Record<string, unknown>
      const model = String(o.model ?? '').trim()
      if (!model) continue
      const pid = String(
        o.providerId ?? (o as { provider_id?: unknown }).provider_id ?? ''
      ).trim()
      out[k] = { providerId: pid || fid, model }
    } else if (typeof v === 'string' && v.trim()) {
      out[k] = { providerId: fid, model: v.trim() }
    }
  }
  return out
}

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
    maxToolRounds: 100,
    maxSubAgentToolRounds: 100,
    rawContentViewEnabled: true,
    debugDumpLlmPrompts: false,
    agentDefaultModels: {}
  })
  const loading = ref(false)
  const testing = ref(false)
  const testResult = ref<{ ok: boolean; latencyMs: number; message: string } | null>(null)

  const activeProvider = computed((): ProviderConfig => {
    const list = settings.value.providers
    if (!list.length) return defaultProviders[0]
    return list.find(p => p.id === settings.value.activeProviderId) ?? list[0]
  })

  const activeBaseUrl = computed(() => activeProvider.value.baseUrl)
  const activeModelList = computed(() => activeProvider.value.models)

  /** Effective reasoning flag for active provider + current `settings.model` (model override wins). */
  const effectiveReasoningInMessages = computed((): boolean => {
    const st = settings.value
    const provs = st.providers
    if (!provs.length) return true
    const p = provs.find(x => x.id === st.activeProviderId) ?? provs[0]
    const model = st.model.trim()
    const over = p.modelConfigs?.[model]
    if (over?.reasoningInMessages !== undefined) return over.reasoningInMessages
    if (p.reasoningInMessages !== undefined) return p.reasoningInMessages
    return true
  })

  const DEFAULT_MODEL_TEMPERATURE = 0.7
  const DEFAULT_MODEL_MAX_TOKENS = 2048

  function modelGenerationFromConfig(
    p: ProviderConfig,
    model: string
  ): { temperature: number; maxTokens: number } {
    const over = p.modelConfigs?.[model.trim()]
    return {
      temperature: over?.temperature ?? stFallbackTemperature(),
      maxTokens: over?.maxTokens ?? stFallbackMaxTokens()
    }
  }

  function stFallbackTemperature(): number {
    const t = settings.value.temperature
    return Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE
  }

  function stFallbackMaxTokens(): number {
    const n = settings.value.maxTokens
    return n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS
  }

  /** Effective temperature for active provider + current model. */
  const effectiveTemperature = computed((): number => {
    const st = settings.value
    const p = st.providers.find(x => x.id === st.activeProviderId) ?? st.providers[0]
    if (!p) return stFallbackTemperature()
    return modelGenerationFromConfig(p, st.model).temperature
  })

  /** Effective max output tokens for active provider + current model. */
  const effectiveMaxTokens = computed((): number => {
    const st = settings.value
    const p = st.providers.find(x => x.id === st.activeProviderId) ?? st.providers[0]
    if (!p) return stFallbackMaxTokens()
    return modelGenerationFromConfig(p, st.model).maxTokens
  })

  /** 所有 provider 的所有模型合并列表（带 provider 标识） */
  const allModels = computed(() => {
    const result: Array<{ model: string; providerId: string; providerName: string }> = []
    for (const p of settings.value.providers) {
      for (const m of p.models) {
        result.push({ model: m, providerId: p.id, providerName: p.name })
      }
    }
    return result
  })

  async function load() {
    loading.value = true
    const s = await getSettings().catch(() => null)
    if (s) {
      const legacy =
        'reasoningInMessages' in s && typeof (s as { reasoningInMessages?: boolean }).reasoningInMessages === 'boolean'
          ? (s as { reasoningInMessages?: boolean }).reasoningInMessages
          : undefined
      const providersNorm = normalizeProviders(s.providers, legacy)

      if (s.providers && s.providers.length > 0) {
        const activeId = s.activeProviderId || providersNorm[0]?.id || 'qwen'
        settings.value = {
          ...s,
          providers: providersNorm,
          workspaceRoot: s.workspaceRoot ?? '',
          leadAgentId: s.leadAgentId ?? '',
          contextCompressionEnabled: s.contextCompressionEnabled ?? true,
          contextBudgetChars: s.contextBudgetChars ?? 120_000,
          contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 6,
          contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
          maxToolRounds: s.maxToolRounds ?? 100,
          maxSubAgentToolRounds: s.maxSubAgentToolRounds ?? s.maxToolRounds ?? 100,
          rawContentViewEnabled: s.rawContentViewEnabled !== false,
          debugDumpLlmPrompts: s.debugDumpLlmPrompts === true,
          agentDefaultModels: normalizeAgentDefaultModels(s.agentDefaultModels as Record<string, unknown>, activeId)
        }
      } else {
        const activeId = s.activeProviderId || providersNorm[0]?.id || 'qwen'
        settings.value = {
          ...settings.value,
          ...s,
          providers: providersNorm,
          activeProviderId: activeId,
          workspaceRoot: s.workspaceRoot ?? '',
          leadAgentId: s.leadAgentId ?? '',
          contextCompressionEnabled: s.contextCompressionEnabled ?? true,
          contextBudgetChars: s.contextBudgetChars ?? 120_000,
          contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 6,
          contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
          maxToolRounds: s.maxToolRounds ?? 100,
          maxSubAgentToolRounds: s.maxSubAgentToolRounds ?? s.maxToolRounds ?? 100,
          rawContentViewEnabled: s.rawContentViewEnabled !== false,
          debugDumpLlmPrompts: s.debugDumpLlmPrompts === true,
          agentDefaultModels: normalizeAgentDefaultModels(s.agentDefaultModels as Record<string, unknown>, activeId)
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
    merged.agentDefaultModels = normalizeAgentDefaultModels(
      merged.agentDefaultModels as Record<string, unknown>,
      merged.activeProviderId
    )
    const updated = await updateSettings(merged)
    settings.value = {
      ...updated,
      agentDefaultModels: normalizeAgentDefaultModels(
        updated.agentDefaultModels as Record<string, unknown>,
        updated.activeProviderId
      )
    }
  }

  async function setActiveProvider(id: string) {
    const provider = settings.value.providers.find(p => p.id === id)
    if (provider) {
      settings.value.activeProviderId = id
      if (!provider.models.includes(settings.value.model)) {
        settings.value.model = provider.models[0]
      }
      // Sync the new provider's API key to backend storage and persist settings
      if (provider.apiKey) {
        await saveProviderKey(id, provider.apiKey)
      } else {
        // Clear key.dat when switching to a provider without a stored key
        // so the backend won't use a stale key from the previous provider
        await clearApiKey()
        settings.value.hasKey = false
      }
      await save({ activeProviderId: id, model: settings.value.model })
    }
  }

  function addProvider(provider: ProviderConfig) {
    settings.value.providers.push(provider)
    settings.value.activeProviderId = provider.id
    if (!provider.models.includes(settings.value.model)) {
      settings.value.model = provider.models[0] || settings.value.model
    }
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

  function getAgentDefaultModelRef(agentId: string): AgentModelRef | undefined {
    return settings.value.agentDefaultModels[agentId]
  }

  async function setAgentDefaultModel(agentId: string, ref: AgentModelRef | null) {
    const next = { ...settings.value.agentDefaultModels }
    if (!ref || !ref.model?.trim()) {
      delete next[agentId]
    } else {
      const pid = (ref.providerId || '').trim() || settings.value.activeProviderId
      next[agentId] = { providerId: pid, model: ref.model.trim() }
    }
    await save({ agentDefaultModels: next })
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
    effectiveReasoningInMessages, effectiveTemperature, effectiveMaxTokens, allModels,
    load, save, setActiveProvider, addProvider, updateProvider, removeProvider,
    saveKey, removeKey, runTest,
    getAgentDefaultModelRef, setAgentDefaultModel
  }
})
