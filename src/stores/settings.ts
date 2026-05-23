import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getSettings,
  updateSettings,
  updateUserSettings,
  updatePlatformSettings,
  setApiKey,
  clearApiKey,
  testConnection
} from '../lib/api'
import type {
  AgentModelRef,
  ComputerInitialTier,
  EffectiveSettingsView,
  ModelSettings,
  PlatformSettings,
  ProviderConfig,
  ThemePreference,
  UserSettings
} from '../types/chat'
import { applyTheme } from '../lib/theme'
import {
  DEFAULT_MODEL_MAX_TOKENS,
  DEFAULT_MODEL_TEMPERATURE,
  pruneInheritedModelConfigs
} from '../composables/useRuntimeParams'

const defaultPlatformSettings = (): PlatformSettings => ({
  providers: defaultProviders,
  activeProviderId: 'qwen',
  model: 'qwen3.5-plus',
  temperature: 0.3,
  maxTokens: 64_000,
  toolApprovalMode: 'auto',
  agentMode: 'single',
  workspaceRoot: '',
  leadAgentId: '',
  contextCompressionEnabled: true,
  contextBudgetChars: 100_000,
  contextKeepRecentUserTurns: 3,
  contextSummaryMaxTokens: 1024,
  maxToolRounds: 200,
  maxSubAgentToolRounds: 200,
  rawContentViewEnabled: false,
  debugDumpLlmPrompts: false,
  agentDefaultModels: {},
  agentTaskBoardHistoryTrim: {},
  computerHumanLike: false,
  computerInitialTier: 'primary',
  computerAnnotatedScreenViewEnabled: false,
  agentUiOverrides: {},
  computerTierLlm: {
    primary: { providerId: 'qwen', model: 'qwen3.5-plus', enableThinking: true, thinkingBudget: 2048 },
    intermediate: { providerId: 'qwen', model: 'qwen3.5-plus', enableThinking: true, thinkingBudget: 2048 },
    advanced: { providerId: 'qwen', model: 'qwen3.6-plus', enableThinking: true, thinkingBudget: 8192 }
  }
})

function normalizeMergedSettings(s: ModelSettings, activeId: string): ModelSettings {
  const providersNorm = normalizeProviders(s.providers, undefined, globalGenFallbackFrom(s))
  return {
    ...s,
    providers: providersNorm,
    workspaceRoot: s.workspaceRoot ?? '',
    leadAgentId: s.leadAgentId ?? '',
    contextCompressionEnabled: s.contextCompressionEnabled ?? true,
    contextBudgetChars: s.contextBudgetChars ?? 100_000,
    contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 3,
    contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
    maxToolRounds: s.maxToolRounds ?? 200,
    maxSubAgentToolRounds: s.maxSubAgentToolRounds ?? s.maxToolRounds ?? 200,
    rawContentViewEnabled: s.rawContentViewEnabled === true,
    debugDumpLlmPrompts: s.debugDumpLlmPrompts === true,
    agentDefaultModels: normalizeAgentDefaultModels(s.agentDefaultModels as Record<string, unknown>, activeId),
    agentTaskBoardHistoryTrim: { ...(s.agentTaskBoardHistoryTrim ?? {}) },
    computerHumanLike: s.computerHumanLike === true,
    computerInitialTier: normalizeComputerInitialTier(s.computerInitialTier),
    computerAnnotatedScreenViewEnabled: s.computerAnnotatedScreenViewEnabled === true,
    theme: (s.theme as ThemePreference) ?? 'system',
    agentUiOverrides: { ...(s.agentUiOverrides ?? {}) }
  }
}

function globalGenFallbackFrom(st?: Pick<ModelSettings, 'temperature' | 'maxTokens'>) {
  return {
    temperature: () => st?.temperature ?? 0.3,
    maxTokens: () => st?.maxTokens ?? 64_000
  }
}

const defaultProviders: ProviderConfig[] = [
  {
    id: 'qwen',
    name: '千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: '',
    models: [
      'qwen3.5-plus',
      'qwen3.5-27b',
      'qwen3.5-flash',
      'qwen3.7-max',
      'qwen3.6-plus',
      'qwen3.6-27b',
      'qwen3.6-flash'
    ],
    reasoningInMessages: false,
    enableThinking: true,
    thinkingBudget: 2048,
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
function normalizeProvider(
  p: ProviderConfig,
  legacyReasoning?: boolean,
  globalFallback?: { temperature: () => number; maxTokens: () => number }
): ProviderConfig {
  const base: ProviderConfig = {
    ...p,
    // 旧数据或异常响应可能缺 models；设置页模板会读 models.length，必须是数组。
    models: Array.isArray(p.models) ? [...p.models] : [],
    modelConfigs: p.modelConfigs ? { ...p.modelConfigs } : {},
    reasoningInMessages:
      p.reasoningInMessages !== undefined
        ? p.reasoningInMessages
        : legacyReasoning !== undefined
          ? legacyReasoning
          : undefined
  }
  const fallback = globalFallback ?? {
    temperature: () => DEFAULT_MODEL_TEMPERATURE,
    maxTokens: () => DEFAULT_MODEL_MAX_TOKENS
  }
  return {
    ...base,
    modelConfigs: pruneInheritedModelConfigs(base, base.modelConfigs, fallback)
  }
}

function normalizeComputerInitialTier(v: unknown): ComputerInitialTier {
  const t = typeof v === 'string' ? v.trim().toLowerCase() : ''
  if (t === 'intermediate' || t === 'mid' || t === 'medium') return 'intermediate'
  if (t === 'advanced' || t === 'high') return 'advanced'
  return 'primary'
}

function normalizeProviders(
  list: ProviderConfig[] | undefined,
  legacyReasoning?: boolean,
  globalFallback?: { temperature: () => number; maxTokens: () => number }
): ProviderConfig[] {
  const raw = list?.length ? list : defaultProviders
  return raw.map(p => normalizeProvider(p, legacyReasoning, globalFallback))
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
  const userSettings = ref<UserSettings>({ theme: 'system' })
  const platformSettings = ref<PlatformSettings>(defaultPlatformSettings())
  const settings = ref<ModelSettings>({
    ...defaultPlatformSettings(),
    hasKey: false,
    theme: 'system'
  } as ModelSettings)
  const canEditPlatform = ref(false)
  const isPlatformAdmin = ref(false)
  const loading = ref(false)
  const testing = ref(false)
  const testResult = ref<{ ok: boolean; latencyMs: number; message: string } | null>(null)

  function applyEffectiveView(view: EffectiveSettingsView) {
    userSettings.value = { ...view.user, theme: (view.user.theme as ThemePreference) ?? 'system' }
    platformSettings.value = {
      ...defaultPlatformSettings(),
      ...view.platform,
      providers: normalizeProviders(view.platform.providers, undefined, globalGenFallbackFrom(view.merged)),
      computerTierLlm: { ...defaultPlatformSettings().computerTierLlm, ...view.platform.computerTierLlm }
    }
    canEditPlatform.value = view.canEditPlatform
    isPlatformAdmin.value = view.isPlatformAdmin
    const activeId = view.merged.activeProviderId || 'qwen'
    settings.value = normalizeMergedSettings(view.merged, activeId)
    applyTheme(settings.value.theme)
  }

  function storeGlobalGenFallbackFrom(st?: Pick<ModelSettings, 'temperature' | 'maxTokens'>) {
    return {
      temperature: () => st?.temperature ?? settings.value.temperature,
      maxTokens: () => st?.maxTokens ?? settings.value.maxTokens
    }
  }

  const activeProvider = computed((): ProviderConfig => {
    const list = settings.value.providers
    if (!list.length) return defaultProviders[0]
    return list.find(p => p.id === settings.value.activeProviderId) ?? list[0]
  })

  const activeBaseUrl = computed(() => activeProvider.value.baseUrl)
  const activeModelList = computed(() => activeProvider.value.models ?? [])

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
      temperature: over?.temperature ?? p.temperature ?? stFallbackTemperature(),
      maxTokens: over?.maxTokens ?? p.maxTokens ?? stFallbackMaxTokens()
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
      for (const m of p.models ?? []) {
        result.push({ model: m, providerId: p.id, providerName: p.name })
      }
    }
    return result
  })

  async function load() {
    loading.value = true
    const view = await getSettings().catch(() => null)
    if (view) {
      applyEffectiveView(view)
    }
    loading.value = false
  }

  async function saveUser(patch: Partial<UserSettings>) {
    if (patch.theme !== undefined) applyTheme(patch.theme)
    const next: UserSettings = { ...userSettings.value, ...patch }
    const view = await updateUserSettings(next)
    applyEffectiveView(view)
  }

  async function savePlatform(patch: Partial<PlatformSettings>) {
    const merged: PlatformSettings = { ...platformSettings.value, ...patch }
    const view = await updatePlatformSettings(merged)
    applyEffectiveView(view)
  }

  async function save(patch: Partial<ModelSettings> & Pick<Partial<PlatformSettings>, 'computerTierLlm'>) {
    if (patch.theme !== undefined) {
      await saveUser({ theme: patch.theme })
      patch = { ...patch }
      delete patch.theme
    }
    if (Object.keys(patch).length === 0) return
    const merged: ModelSettings = { ...settings.value, ...patch }
    merged.agentDefaultModels = normalizeAgentDefaultModels(
      merged.agentDefaultModels as Record<string, unknown>,
      merged.activeProviderId
    )
    if (canEditPlatform.value) {
      const extra = patch as Partial<ModelSettings> & {
        computerTierLlm?: PlatformSettings['computerTierLlm']
      }
      const platformPatch: PlatformSettings = {
        ...platformSettings.value,
        providers: merged.providers,
        activeProviderId: merged.activeProviderId,
        model: merged.model,
        temperature: merged.temperature,
        maxTokens: merged.maxTokens,
        toolApprovalMode: merged.toolApprovalMode,
        agentMode: merged.agentMode,
        workspaceRoot: merged.workspaceRoot,
        leadAgentId: merged.leadAgentId,
        contextCompressionEnabled: merged.contextCompressionEnabled,
        contextBudgetChars: merged.contextBudgetChars,
        contextKeepRecentUserTurns: merged.contextKeepRecentUserTurns,
        contextSummaryMaxTokens: merged.contextSummaryMaxTokens,
        maxToolRounds: merged.maxToolRounds,
        maxSubAgentToolRounds: merged.maxSubAgentToolRounds,
        rawContentViewEnabled: merged.rawContentViewEnabled,
        debugDumpLlmPrompts: merged.debugDumpLlmPrompts,
        agentDefaultModels: merged.agentDefaultModels,
        agentTaskBoardHistoryTrim: merged.agentTaskBoardHistoryTrim,
        computerHumanLike: merged.computerHumanLike,
        computerInitialTier: merged.computerInitialTier,
        computerAnnotatedScreenViewEnabled: merged.computerAnnotatedScreenViewEnabled,
        agentUiOverrides: merged.agentUiOverrides,
        computerTierLlm: extra.computerTierLlm ?? platformSettings.value.computerTierLlm
      }
      await savePlatform(platformPatch)
    } else {
      const view = await updateSettings(merged)
      applyEffectiveView(view)
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
    const entry = normalizeProvider(provider, undefined, storeGlobalGenFallbackFrom())
    // 用新数组 append，勿 push 编辑中的同一对象引用，避免与 editingProvider 草稿互相污染。
    settings.value.providers = [...settings.value.providers, entry]
    settings.value.activeProviderId = entry.id
    const models = entry.models ?? []
    if (!models.includes(settings.value.model)) {
      settings.value.model = models[0] || settings.value.model
    }
  }

  function updateProvider(id: string, patch: Partial<ProviderConfig>) {
    const i = settings.value.providers.findIndex(p => p.id === id)
    if (i < 0) return
    const prev = settings.value.providers[i]
    // 勿 Object.assign(provider, patch)：嵌套 modelConfigs 在 Pinia 下可能不触发列表更新。
    // 须替换 providers[i] 并赋新数组，保证设置页服务商列表与编辑区同步刷新。
    const next = normalizeProvider(
      {
        ...prev,
        ...patch,
        models: patch.models ?? prev.models ?? [],
        modelConfigs:
          patch.modelConfigs !== undefined
            ? { ...patch.modelConfigs }
            : { ...(prev.modelConfigs ?? {}) }
      },
      undefined,
      storeGlobalGenFallbackFrom()
    )
    const list = [...settings.value.providers]
    list[i] = next
    settings.value.providers = list
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

  function defaultTaskBoardHistoryTrim(agentId: string): boolean {
    return agentId.trim() === 'computer'
  }

  function isTaskBoardHistoryTrimEnabled(agentId: string): boolean {
    const id = agentId.trim() || 'default'
    const v = settings.value.agentTaskBoardHistoryTrim?.[id]
    if (v !== undefined) return v
    return defaultTaskBoardHistoryTrim(id)
  }

  async function setTaskBoardHistoryTrim(agentId: string, enabled: boolean) {
    const id = agentId.trim() || 'default'
    const next = { ...(settings.value.agentTaskBoardHistoryTrim ?? {}) }
    next[id] = enabled
    await save({ agentTaskBoardHistoryTrim: next })
  }

  function isComputerHumanLikeEnabled(): boolean {
    return settings.value.computerHumanLike === true
  }

  async function setComputerHumanLike(enabled: boolean) {
    await save({ computerHumanLike: enabled })
  }

  async function setComputerInitialTier(tier: ComputerInitialTier) {
    await save({ computerInitialTier: tier })
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
    settings,
    userSettings,
    platformSettings,
    canEditPlatform,
    isPlatformAdmin,
    loading,
    testing,
    testResult,
    activeProvider,
    activeBaseUrl,
    activeModelList,
    effectiveReasoningInMessages,
    effectiveTemperature,
    effectiveMaxTokens,
    allModels,
    load,
    save,
    saveUser,
    savePlatform,
    setActiveProvider,
    addProvider,
    updateProvider,
    removeProvider,
    saveKey,
    removeKey,
    runTest,
    getAgentDefaultModelRef,
    setAgentDefaultModel,
    isTaskBoardHistoryTrimEnabled,
    setTaskBoardHistoryTrim,
    defaultTaskBoardHistoryTrim,
    isComputerHumanLikeEnabled,
    setComputerHumanLike,
    setComputerInitialTier
  }
})
