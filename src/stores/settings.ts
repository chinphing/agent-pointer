import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getSettings,
  updateSettings,
  updateAgentSettings,
  updateDebugSessionSettings,
  updateUserSettings,
  setApiKey,
  clearApiKey,
  testConnection
} from '../lib/api'
import type {
  AgentModelRef,
  AgentUiConfig,
  ComputerInitialTier,
  ComputerPipelineLlmSettings,
  ComputerTierLlmConfig,
  DebugSessionSettings,
  EffectiveSettingsView,
  MediaModelOverrides,
  MediaUnderstandingModes,
  ModelSettings,
  PerformanceMode,
  PlatformSettings,
  ProviderConfig,
  ThemePreference,
  UserSettings
} from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { GENERAL_AGENT_ID } from '../lib/agentUi'
import { applyTheme } from '../lib/theme'
import {
  DOUBAO_GENERATION_MODELS,
  modelCanGenerateImage,
  modelCanGenerateVideo,
  modelSupportsAudioTranscription,
  modelSupportsVision,
  QWEN_AUDIO_TRANSCRIPTION_MODELS,
  QWEN_GENERATION_MODELS,
  seedProviderModelCapabilities
} from '../lib/modelCapabilities'
import {
  DEFAULT_MODEL_MAX_TOKENS,
  DEFAULT_MODEL_TEMPERATURE,
  pruneInheritedModelConfigs
} from '../composables/useRuntimeParams'

function defaultModeLlm(
  providerId: string,
  fast: string,
  standard: string,
  expert: string
): Record<PerformanceMode, ComputerTierLlmConfig> {
  return {
    fast: { providerId, model: fast, enableThinking: true, thinkingBudget: 2048 },
    standard: { providerId, model: standard, enableThinking: true, thinkingBudget: 2048 },
    expert: { providerId, model: expert, enableThinking: true, thinkingBudget: 8192 }
  }
}

const defaultAgentModeLlm = () => ({
  general: {
    fast: { providerId: 'deepseek', model: 'deepseek-v4-flash', enableThinking: true, thinkingBudget: 2048 },
    standard: { providerId: 'deepseek', model: 'deepseek-v4-pro', enableThinking: true, thinkingBudget: 2048 },
    expert: { providerId: 'qwen', model: 'qwen3.7-plus', enableThinking: true, thinkingBudget: 8192 }
  },
  coder: {
    fast: { providerId: 'deepseek', model: 'deepseek-v4-flash', enableThinking: true, thinkingBudget: 2048 },
    standard: { providerId: 'deepseek', model: 'deepseek-v4-pro', enableThinking: true, thinkingBudget: 2048 },
    expert: { providerId: 'qwen', model: 'qwen3.7-max', enableThinking: true, thinkingBudget: 8192 }
  }
})

const defaultMediaModeLlm = () => ({
  image: defaultModeLlm('qwen', 'qwen3.5-flash', 'qwen3.5-plus', 'qwen3.6-plus'),
  audio: defaultModeLlm('qwen', 'qwen3-asr-flash', 'fun-asr', 'fun-asr'),
  video: defaultModeLlm('qwen', 'qwen3.5-flash', 'qwen3.5-plus', 'qwen3.6-plus')
})

const defaultPlatformSettings = (): PlatformSettings => ({
  providers: defaultProviders,
  mediaOss: undefined,
  datiApiUrl: '',
  datiAuthcode: '',
  datiTypeno: '',
  datiAuthor: ''
})

function migratePlannerSettingsFields(
  s: ModelSettings & {
    taskBoardPlannerEnabled?: boolean
    taskBoardWorkItemsEnabled?: boolean
    taskBoardComputerNoExecInit?: boolean
    computerStandalonePlannerEnabled?: boolean
  }
): ModelSettings {
  const raw = s as unknown as Record<string, unknown>
  const {
    taskBoardPlannerEnabled: _p,
    taskBoardWorkItemsEnabled: _w,
    taskBoardComputerNoExecInit: _n,
    computerStandalonePlannerEnabled: _csp,
    ...rest
  } = raw
  return rest as unknown as ModelSettings
}

function normalizeMergedSettings(s: ModelSettings, activeId: string): ModelSettings {
  const migrated = migratePlannerSettingsFields(s)
  const providersNorm = normalizeProviders(migrated.providers, undefined, globalGenFallbackFrom(migrated))
  return {
    ...migrated,
    providers: providersNorm,
    workspaceRoot: s.workspaceRoot ?? '',
    leadAgentId: (s.leadAgentId ?? '').trim() || DEFAULT_LEAD_AGENT_ID,
    contextCompressionEnabled: s.contextCompressionEnabled ?? true,
    contextBudgetTokens:
      s.contextBudgetTokens ?? (s as { contextBudgetChars?: number }).contextBudgetChars ?? 100_000,
    contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 3,
    contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
    maxToolRounds: s.maxToolRounds ?? 200,
    maxSubAgentToolRounds: s.maxSubAgentToolRounds ?? s.maxToolRounds ?? 200,
    maxSubAgentSpawnDepth: s.maxSubAgentSpawnDepth ?? 2,
    rawContentViewEnabled: s.rawContentViewEnabled === true,
    debugDumpLlmPrompts: s.debugDumpLlmPrompts === true,
    terminalEnvOverrides: { ...(s.terminalEnvOverrides ?? {}) },
    debugMenusEnabled: s.debugMenusEnabled === true,
    taskBoardShowChildBoards: migrated.taskBoardShowChildBoards === true,
    agentDefaultModels: normalizeAgentDefaultModels(s.agentDefaultModels as Record<string, unknown>, activeId),
    agentTaskBoardHistoryTrim: { ...(s.agentTaskBoardHistoryTrim ?? {}) },
    computerHumanLike: s.computerHumanLike === true,
    computerInitialTier: normalizeComputerInitialTier(s.computerInitialTier),
    computerAutoSwitchMonitor: s.computerAutoSwitchMonitor !== false,
    computerAnnotatedScreenViewEnabled: s.computerAnnotatedScreenViewEnabled === true,
    captchaSliderOffsetPx: Number.isFinite(Number(s.captchaSliderOffsetPx)) ? Number(s.captchaSliderOffsetPx) : 0,
    theme: (s.theme as ThemePreference) ?? 'system',
    agentUiOverrides: { ...(s.agentUiOverrides ?? {}) },
    mediaModelOverrides: { ...(s.mediaModelOverrides ?? {}) },
    agentPerformanceModes: { ...(s.agentPerformanceModes ?? {}) },
    mediaUnderstandingModes: { ...(s.mediaUnderstandingModes ?? {}) },
    agentModeLlm: { ...(s.agentModeLlm ?? {}) },
    mediaModeLlm: { ...(s.mediaModeLlm ?? {}) },
    parallelToolExecutionEnabled: s.parallelToolExecutionEnabled !== false,
    maxParallelToolCalls: s.maxParallelToolCalls,
    maxParallelSubAgents: s.maxParallelSubAgents,
    maxParallelMediaJobs: s.maxParallelMediaJobs,
    maxConcurrentRuns: Number.isFinite(Number(s.maxConcurrentRuns)) && Number(s.maxConcurrentRuns) >= 1
      ? Math.floor(Number(s.maxConcurrentRuns))
      : 4
  }
}

function globalGenFallbackFrom(st?: Pick<ModelSettings, 'temperature' | 'maxTokens'>) {
  return {
    temperature: () => st?.temperature ?? 0.3,
    maxTokens: () => st?.maxTokens ?? 64_000
  }
}

/**
 * WEB non-admin responses omit `terminalEnvOverrides`. Keep the in-memory map when
 * the field is absent. Empty `{}` from an authoritative session save is kept as-is
 * (callers clear the local draft before save when deleting all rows).
 */
function retainTerminalEnvOverrides(
  mergedIn?: Record<string, string>,
  prevMerged?: Record<string, string>
): Record<string, string> {
  const previous = prevMerged ?? {}
  if (mergedIn === undefined) return { ...previous }
  return { ...mergedIn }
}

const defaultProviders: ProviderConfig[] = [
  seedProviderModelCapabilities({
    id: 'qwen',
    name: '千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: '',
    models: [
      'qwen3.5-plus',
      'qwen3.5-27b',
      'qwen3.5-flash',
      'qwen3.7-max',
      'qwen3.7-plus',
      'qwen3.6-plus',
      'qwen3.6-27b',
      'qwen3.6-flash',
      ...QWEN_AUDIO_TRANSCRIPTION_MODELS,
      ...QWEN_GENERATION_MODELS
    ],
    reasoningInMessages: false,
    enableThinking: true,
    thinkingBudget: 2048,
    modelConfigs: {}
  }),
  seedProviderModelCapabilities({
    id: 'deepseek',
    name: '深度求索',
    baseUrl: 'https://api.deepseek.com/v1',
    apiKey: '',
    models: ['deepseek-v4-flash', 'deepseek-v4-pro'],
    reasoningInMessages: true,
    modelConfigs: {}
  }),
  seedProviderModelCapabilities({
    id: 'doubao',
    name: '豆包',
    baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
    apiKey: '',
    models: [...DOUBAO_GENERATION_MODELS],
    modelConfigs: {}
  })
]

/** Normalize provider entries from API; merge legacy root `reasoningInMessages` when per-provider value is absent. */
function normalizeProvider(
  p: ProviderConfig,
  legacyReasoning?: boolean,
  globalFallback?: { temperature: () => number; maxTokens: () => number }
): ProviderConfig {
  const base: ProviderConfig = seedProviderModelCapabilities({
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
  })
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
  const userSettings = ref<UserSettings>({
    theme: 'system',
    agentSkillOverrides: {}
  })
  const platformSettings = ref<PlatformSettings>(defaultPlatformSettings())
  const settings = ref<ModelSettings>({
    ...defaultPlatformSettings(),
    activeProviderId: 'qwen',
    model: 'qwen3.5-plus',
    temperature: 0.3,
    maxTokens: 64_000,
    hasKey: false,
    toolApprovalMode: 'auto',
    agentMode: 'single',
    workspaceRoot: '',
    leadAgentId: 'general',
    contextCompressionEnabled: true,
    contextBudgetTokens: 100_000,
    contextKeepRecentUserTurns: 3,
    contextSummaryMaxTokens: 1024,
    maxToolRounds: 200,
    maxSubAgentToolRounds: 200,
    maxSubAgentSpawnDepth: 2,
    rawContentViewEnabled: false,
    debugDumpLlmPrompts: false,
    terminalEnvOverrides: {},
    debugMenusEnabled: false,
    taskBoardShowChildBoards: false,
    agentDefaultModels: {},
    agentTaskBoardHistoryTrim: {},
    computerHumanLike: true,
    computerInitialTier: 'intermediate',
    computerAutoSwitchMonitor: true,
    computerAnnotatedScreenViewEnabled: false,
    captchaSliderOffsetPx: 0,
    agentUiOverrides: {},
    mediaModelOverrides: {
      image: { providerId: 'qwen', model: 'qwen3.5-plus' },
      audio: { providerId: 'qwen', model: 'qwen3-asr-flash' },
      imageGeneration: { providerId: 'doubao', model: 'doubao-seedream-5-0-lite-260128' },
      videoGeneration: { providerId: 'doubao', model: 'doubao-seedance-2-0-fast-260128' }
    },
    computerTierLlm: {
      primary: { providerId: 'qwen', model: 'qwen3.5-plus', enableThinking: true, thinkingBudget: 2048 },
      intermediate: { providerId: 'qwen', model: 'qwen3.5-plus', enableThinking: true, thinkingBudget: 2048 },
      advanced: { providerId: 'qwen', model: 'qwen3.7-plus', enableThinking: true, thinkingBudget: 8192 }
    },
    computerPipelineLlm: {
      decision: 'qwen3.5-flash',
      position: 'qwen3.5-plus',
      verify: 'qwen3.5-flash',
      decisionProviderId: 'qwen',
      positionProviderId: 'qwen',
      verifyProviderId: 'qwen',
      positionThinkingBudget: 1024,
      verifyThinkingBudget: 256
    },
    agentModeLlm: defaultAgentModeLlm(),
    mediaModeLlm: defaultMediaModeLlm(),
    agentPerformanceModes: { general: 'fast', coder: 'fast' },
    mediaUnderstandingModes: { image: 'fast', audio: 'fast', video: 'fast' },
    parallelToolExecutionEnabled: true,
    theme: 'system'
  } as ModelSettings)
  const canEditPlatform = ref(false)
  const isPlatformAdmin = ref(false)
  const loading = ref(false)
  const testing = ref(false)
  const testResult = ref<{ ok: boolean; latencyMs: number; message: string } | null>(null)

  function applyEffectiveView(view: EffectiveSettingsView) {
    userSettings.value = { ...view.user, theme: (view.user.theme as ThemePreference) ?? 'system' }
    // WEB non-admin responses omit debug / mode-LLM fields. Preserve the current
    // in-memory values when the payload lacks them so save→reopen does not snap
    // back to built-in defaults (admins now receive these fields from the API).
    const platformIn = view.platform ?? ({} as PlatformSettings)
    const mergedIn = view.merged ?? ({} as ModelSettings)
    const prevPlatform = platformSettings.value
    const prevMerged = settings.value
    const retainedTerminalEnv = retainTerminalEnvOverrides(
      mergedIn.terminalEnvOverrides,
      prevMerged.terminalEnvOverrides
    )
    platformSettings.value = {
      ...defaultPlatformSettings(),
      ...platformIn,
      providers: normalizeProviders(platformIn.providers, undefined, globalGenFallbackFrom(mergedIn))
    }
    canEditPlatform.value = view.canEditPlatform
    isPlatformAdmin.value = view.isPlatformAdmin
    const activeId = mergedIn.activeProviderId || 'qwen'
    const nextMerged = {
      ...mergedIn,
      // Keep session debug toggles when WEB omitted them from the response.
      debugMenusEnabled:
        mergedIn.debugMenusEnabled ?? prevMerged.debugMenusEnabled,
      rawContentViewEnabled:
        mergedIn.rawContentViewEnabled ?? prevMerged.rawContentViewEnabled,
      debugDumpLlmPrompts:
        mergedIn.debugDumpLlmPrompts ?? prevMerged.debugDumpLlmPrompts,
      terminalEnvOverrides: { ...retainedTerminalEnv },
      taskBoardShowChildBoards:
        mergedIn.taskBoardShowChildBoards ?? prevMerged.taskBoardShowChildBoards,
      computerAnnotatedScreenViewEnabled:
        mergedIn.computerAnnotatedScreenViewEnabled ??
        prevMerged.computerAnnotatedScreenViewEnabled,
      agentUiOverrides:
        mergedIn.agentUiOverrides ?? prevMerged.agentUiOverrides,
      agentModeLlm: mergedIn.agentModeLlm ?? prevMerged.agentModeLlm ?? {},
      mediaModeLlm: mergedIn.mediaModeLlm ?? prevMerged.mediaModeLlm ?? {},
      computerTierLlm: mergedIn.computerTierLlm ?? prevMerged.computerTierLlm ?? {},
      computerPipelineLlm:
        mergedIn.computerPipelineLlm ?? prevMerged.computerPipelineLlm ?? {}
    }
    settings.value = normalizeMergedSettings(nextMerged, activeId)
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
    if (!provs.length) return false
    const p = provs.find(x => x.id === st.activeProviderId) ?? provs[0]
    const model = st.model.trim()
    const over = p.modelConfigs?.[model]
    if (over?.reasoningInMessages !== undefined) return over.reasoningInMessages
    if (p.reasoningInMessages !== undefined) return p.reasoningInMessages
    return false
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

  const visionModels = computed(() =>
    allModels.value.filter(item =>
      modelSupportsVision(settings.value.providers, item.providerId, item.model)
    )
  )

  const audioModels = computed(() =>
    allModels.value.filter(item =>
      modelSupportsAudioTranscription(settings.value.providers, item.providerId, item.model)
    )
  )

  const imageGenerationModels = computed(() =>
    allModels.value.filter(item =>
      modelCanGenerateImage(settings.value.providers, item.providerId, item.model)
    )
  )

  const videoGenerationModels = computed(() =>
    allModels.value.filter(item =>
      modelCanGenerateVideo(settings.value.providers, item.providerId, item.model)
    )
  )

  async function load() {
    loading.value = true
    const view = await getSettings().catch(() => null)
    if (view) {
      applyEffectiveView(view)
    }
    loading.value = false
  }

  function createUserSnapshot(patch: Partial<UserSettings>): UserSettings {
    return cloneJson({ ...userSettings.value, ...patch })
  }

  async function saveUserSnapshot(snapshot: UserSettings) {
    if (snapshot.theme !== undefined) applyTheme(snapshot.theme)
    const view = await updateUserSettings(cloneJson(snapshot))
    // Only refresh the user slice. A full applyEffectiveView would re-apply the
    // still-stale platform map (often `terminalEnvOverrides: {}`) and wipe session
    // debug drafts captured earlier in the same settings-footer save.
    const user = {
      ...view.user,
      theme: (view.user.theme as ThemePreference) ?? 'system'
    }
    userSettings.value = user
    settings.value = {
      ...settings.value,
      theme: user.theme
    }
    applyTheme(user.theme)
  }

  async function saveUser(patch: Partial<UserSettings>) {
    if (patch.theme !== undefined) applyTheme(patch.theme)
    await saveUserSnapshot(createUserSnapshot(patch))
  }

  function cloneJson<T>(value: T): T {
    return JSON.parse(JSON.stringify(value)) as T
  }

  function createDebugSessionSnapshot(
    patch: Partial<DebugSessionSettings> = {}
  ): DebugSessionSettings {
    return cloneJson({
      providers: settings.value.providers,
      activeProviderId: settings.value.activeProviderId,
      model: settings.value.model,
      temperature: settings.value.temperature,
      maxTokens: settings.value.maxTokens,
      computerTierLlm: settings.value.computerTierLlm ?? {},
      computerPipelineLlm: settings.value.computerPipelineLlm ?? {},
      agentModeLlm: settings.value.agentModeLlm ?? {},
      mediaModeLlm: settings.value.mediaModeLlm ?? {},
      ...patch
    })
  }

  async function saveDebugSession(snapshot: DebugSessionSettings) {
    const applied = await updateDebugSessionSettings(cloneJson(snapshot))
    const activeProvider = applied.providers.find(
      provider => provider.id === applied.activeProviderId
    )
    const hasKey = Boolean(activeProvider?.apiKey?.trim())
    // The dedicated endpoint returns only the memory-scoped debug slice.
    // Merge it locally so WEB responses can retain debug mappings without
    // exposing the rest of the platform settings or provider credentials.
    applyEffectiveView({
      user: cloneJson(userSettings.value),
      platform: { ...platformSettings.value, ...cloneJson(applied) },
      merged: { ...settings.value, ...cloneJson(applied), hasKey },
      canEditPlatform: canEditPlatform.value,
      isPlatformAdmin: isPlatformAdmin.value
    })
  }

  function createSessionSnapshot(patch: Partial<ModelSettings>): ModelSettings {
    const snapshot: ModelSettings = cloneJson({ ...settings.value, ...patch })
    snapshot.agentDefaultModels = normalizeAgentDefaultModels(
      snapshot.agentDefaultModels as Record<string, unknown>,
      snapshot.activeProviderId
    )
    return snapshot
  }

  async function saveSessionSnapshot(snapshot: ModelSettings) {
    const view = await updateSettings(cloneJson(snapshot))
    applyEffectiveView(view)
  }

  async function saveSession(patch: Partial<ModelSettings>) {
    if (patch.theme !== undefined) {
      applyTheme(patch.theme)
      settings.value.theme = patch.theme
    }
    if (Object.keys(patch).length === 0) return
    await saveSessionSnapshot(createSessionSnapshot(patch))
  }

  async function saveAgentPreferencesSnapshot(snapshot: ModelSettings) {
    const view = await updateAgentSettings(cloneJson(snapshot))
    applyEffectiveView(view)
  }

  async function saveAgentPreferences(patch: Partial<ModelSettings>) {
    await saveAgentPreferencesSnapshot(createSessionSnapshot(patch))
  }

  async function saveModelService(patch: Partial<ModelSettings>) {
    // 模型配置统一走 agent-settings（唯一持久化端点）；providers/activeProviderId/
    // model/temperature/maxTokens 随全量快照落盘（apiKey 由后端脱敏）。
    await saveAgentPreferences(patch)
  }

  /** Apply agent UI debug/display overrides in memory (chat reflects immediately; persist via save). */
  function patchAgentUiOverride(agentId: string, patch: Partial<AgentUiConfig>) {
    const id = agentId.trim()
    if (!id) return
    const prev = settings.value.agentUiOverrides ?? {}
    settings.value.agentUiOverrides = {
      ...prev,
      [id]: { ...(prev[id] ?? {}), ...patch }
    }
  }

  async function save(patch: Partial<ModelSettings>) {
    await saveSession(patch)
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
      await saveModelService({
        providers: settings.value.providers,
        activeProviderId: id,
        model: settings.value.model
      })
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
    // Renaming/removing models on the active provider must keep settings.model in the list,
    // or debug-session save is rejected ("active model is not configured for provider").
    if (
      settings.value.activeProviderId === id &&
      next.models.length > 0 &&
      !next.models.includes(settings.value.model)
    ) {
      settings.value.model = next.models[0]
    }
  }

  function removeProvider(id: string) {
    const remaining = settings.value.providers.filter(p => p.id !== id)
    if (settings.value.activeProviderId === id) {
      const replacement = remaining.find(provider => provider.models.length > 0)
      if (!replacement) {
        console.warn('[settings] cannot remove the active provider without a replacement model')
        return
      }
      settings.value.activeProviderId = replacement.id
      if (!replacement.models.includes(settings.value.model)) {
        settings.value.model = replacement.models[0]
      }
    }
    settings.value.providers = remaining
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
    const id = agentId.trim() || GENERAL_AGENT_ID
    const v = settings.value.agentTaskBoardHistoryTrim?.[id]
    if (v !== undefined) return v
    return defaultTaskBoardHistoryTrim(id)
  }

  async function setTaskBoardHistoryTrim(agentId: string, enabled: boolean) {
    const id = agentId.trim() || GENERAL_AGENT_ID
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
    // 默认模型是用户配置，走持久化端点（agent-settings），重启保留。
    await saveAgentPreferences({ agentDefaultModels: next })
  }

  function getMediaModelOverride(kind: keyof MediaModelOverrides): AgentModelRef | undefined {
    return settings.value.mediaModelOverrides?.[kind]
  }

  async function setAgentPerformanceMode(agentId: string, mode: PerformanceMode) {
    const next = { ...(settings.value.agentPerformanceModes ?? {}) }
    next[agentId] = mode
    await saveAgentPreferences({ agentPerformanceModes: next })
  }

  function getAgentPerformanceMode(agentId: string): PerformanceMode {
    return settings.value.agentPerformanceModes?.[agentId] ?? 'fast'
  }

  async function setMediaUnderstandingMode(
    kind: keyof MediaUnderstandingModes,
    mode: PerformanceMode
  ) {
    const next: MediaUnderstandingModes = { ...(settings.value.mediaUnderstandingModes ?? {}) }
    next[kind] = mode
    await saveAgentPreferences({ mediaUnderstandingModes: next })
  }

  function getMediaUnderstandingMode(kind: keyof MediaUnderstandingModes): PerformanceMode {
    return settings.value.mediaUnderstandingModes?.[kind] ?? 'fast'
  }

  async function setMediaModelOverride(
    kind: keyof MediaModelOverrides,
    ref: AgentModelRef | null
  ) {
    const next: MediaModelOverrides = { ...(settings.value.mediaModelOverrides ?? {}) }
    if (!ref || !ref.model?.trim()) {
      delete next[kind]
    } else {
      const pid = (ref.providerId || '').trim() || settings.value.activeProviderId
      next[kind] = { providerId: pid, model: ref.model.trim() }
    }
    await saveAgentPreferences({ mediaModelOverrides: next })
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
    visionModels,
    audioModels,
    imageGenerationModels,
    videoGenerationModels,
    load,
    save,
    createUserSnapshot,
    saveUserSnapshot,
    createSessionSnapshot,
    saveSessionSnapshot,
    saveSession,
    saveAgentPreferencesSnapshot,
    saveAgentPreferences,
    createDebugSessionSnapshot,
    saveDebugSession,
    saveModelService,
    saveUser,
    setActiveProvider,
    addProvider,
    updateProvider,
    removeProvider,
    saveKey,
    removeKey,
    runTest,
    getAgentDefaultModelRef,
    setAgentDefaultModel,
    setAgentPerformanceMode,
    getAgentPerformanceMode,
    getMediaModelOverride,
    setMediaModelOverride,
    setMediaUnderstandingMode,
    getMediaUnderstandingMode,
    isTaskBoardHistoryTrimEnabled,
    setTaskBoardHistoryTrim,
    defaultTaskBoardHistoryTrim,
    isComputerHumanLikeEnabled,
    setComputerHumanLike,
    setComputerInitialTier,
    patchAgentUiOverride
  }
})
