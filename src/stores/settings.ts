import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getSettings,
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
  modelCanGenerateImage,
  modelCanGenerateVideo,
  modelSupportsAudioTranscription,
  modelSupportsVision
} from '../lib/modelCapabilities'
import {
  DEFAULT_CONTEXT_BUDGET_TOKENS,
  DEFAULT_MODEL_MAX_TOKENS,
  DEFAULT_MODEL_TEMPERATURE,
  clampContextBudgetTokens,
  pruneInheritedModelConfigs,
  type RuntimeGenFallback
} from '../composables/useRuntimeParams'
import { normalizePlatformProviderTemplates, mergePlatformModelConfigs } from '../lib/platformTierDefaults'

// 场景档位默认由平台目录下发（tierDefaults）；本地不内置任何平台模型固定配置。
const defaultAgentModeLlm = () => ({})
const defaultMediaModeLlm = () => ({})

const defaultPlatformSettings = (): PlatformSettings => ({
  providers: defaultProviders,
  modelCatalog: {},
  tierDefaults: {},
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

const DEFAULT_TOOL_ROUNDS = 5000
const DEFAULT_SUB_AGENT_TOOL_ROUNDS = 200
const CEILING_SUB_AGENT_TOOL_ROUNDS = 200
const LEGACY_TOOL_ROUNDS = new Set([100, 200])

function normalizeToolRounds(raw?: number): number {
  if (!Number.isFinite(Number(raw)) || Number(raw) < 1) return DEFAULT_TOOL_ROUNDS
  const rounds = Math.floor(Number(raw))
  if (LEGACY_TOOL_ROUNDS.has(rounds)) return DEFAULT_TOOL_ROUNDS
  return rounds
}

function normalizeSubAgentToolRounds(raw?: number): number {
  if (!Number.isFinite(Number(raw)) || Number(raw) < 1) return DEFAULT_SUB_AGENT_TOOL_ROUNDS
  return Math.min(Math.floor(Number(raw)), CEILING_SUB_AGENT_TOOL_ROUNDS)
}

function normalizeContextBudgetTokens(raw?: number): number {
  if (!Number.isFinite(Number(raw)) || Number(raw) <= 0) return DEFAULT_CONTEXT_BUDGET_TOKENS
  return clampContextBudgetTokens(Math.floor(Number(raw)))
}

function normalizeMergedSettings(s: ModelSettings, activeId: string): ModelSettings {
  const migrated = migratePlannerSettingsFields(s)
  const contextBudgetTokens = normalizeContextBudgetTokens(
    s.contextBudgetTokens ?? (s as { contextBudgetChars?: number }).contextBudgetChars
  )
  const providersNorm = normalizeProviders(
    migrated.providers,
    undefined,
    globalGenFallbackFrom({ ...migrated, contextBudgetTokens })
  )
  return {
    ...migrated,
    providers: providersNorm,
    workspaceRoot: s.workspaceRoot ?? '',
    leadAgentId: (s.leadAgentId ?? '').trim() || DEFAULT_LEAD_AGENT_ID,
    contextCompressionEnabled: true,
    contextBudgetTokens,
    contextKeepRecentUserTurns: s.contextKeepRecentUserTurns ?? 3,
    contextSummaryMaxTokens: s.contextSummaryMaxTokens ?? 1024,
    maxToolRounds: normalizeToolRounds(s.maxToolRounds),
    fileReadMaxBytes: s.fileReadMaxBytes ?? 65_536,
    fileLineMaxBytes: s.fileLineMaxBytes ?? 1024,
    fileGrepMaxResults: s.fileGrepMaxResults ?? 50,
    terminalOutputMaxBytes: s.terminalOutputMaxBytes ?? 16_384,
    terminalTimeoutSeconds: s.terminalTimeoutSeconds ?? 30,
    terminalMaxWallHours: s.terminalMaxWallHours ?? 24,
    attachmentUploadMaxBytes: s.attachmentUploadMaxBytes ?? 100 * 1024 * 1024,
    maxSubAgentToolRounds: normalizeSubAgentToolRounds(s.maxSubAgentToolRounds),
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

function globalGenFallbackFrom(st?: Pick<ModelSettings, 'temperature' | 'maxTokens' | 'contextBudgetTokens'>): RuntimeGenFallback {
  return {
    temperature: () => st?.temperature ?? DEFAULT_MODEL_TEMPERATURE,
    maxTokens: () => st?.maxTokens ?? 64_000,
    contextBudgetTokens: () => st?.contextBudgetTokens ?? DEFAULT_CONTEXT_BUDGET_TOKENS
  }
}

/** 兜底：后端尚未把平台模板建成 provider 时，由前端按 platformProviders 构建平台服务商。 */
function platformTemplateProviders(
  templates?: PlatformSettings['platformProviders']
): ProviderConfig[] {
  return normalizePlatformProviderTemplates(templates)
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

// 平台服务商完全由平台目录下发；本地不再内置任何平台模型固定配置。
const defaultProviders: ProviderConfig[] = []

/** Normalize provider entries from API; merge legacy root `reasoningInMessages` when per-provider value is absent. */
function normalizeProvider(
  p: ProviderConfig,
  legacyReasoning?: boolean,
  globalFallback?: RuntimeGenFallback
): ProviderConfig {
  const modelConfigs: NonNullable<ProviderConfig['modelConfigs']> = {}
  for (const [id, over] of Object.entries(p.modelConfigs ?? {})) {
    modelConfigs[id] =
      over.contextBudgetTokens !== undefined
        ? { ...over, contextBudgetTokens: clampContextBudgetTokens(over.contextBudgetTokens) }
        : { ...over }
  }
  const base: ProviderConfig = {
    ...p,
    // 旧数据或异常响应可能缺 models；设置页模板会读 models.length，必须是数组。
    models: Array.isArray(p.models) ? [...p.models] : [],
    modelConfigs,
    contextBudgetTokens:
      p.contextBudgetTokens !== undefined
        ? clampContextBudgetTokens(p.contextBudgetTokens)
        : p.contextBudgetTokens,
    reasoningInMessages:
      p.reasoningInMessages !== undefined
        ? p.reasoningInMessages
        : legacyReasoning !== undefined
          ? legacyReasoning
          : undefined
  }
  const fallback = globalFallback ?? {
    temperature: () => DEFAULT_MODEL_TEMPERATURE,
    maxTokens: () => DEFAULT_MODEL_MAX_TOKENS,
    contextBudgetTokens: () => DEFAULT_CONTEXT_BUDGET_TOKENS
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
  globalFallback?: RuntimeGenFallback
): ProviderConfig[] {
  const raw = list ?? []
  return raw.map(p => normalizeProvider(p, legacyReasoning, globalFallback))
}

function normalizeAgentDefaultModels(
  raw: Record<string, AgentModelRef> | Record<string, unknown> | undefined,
  activeProviderId: string
): Record<string, AgentModelRef> {
  const fid = activeProviderId.trim()
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
    activeProviderId: '',
    model: '',
    temperature: DEFAULT_MODEL_TEMPERATURE,
    maxTokens: 64_000,
    hasKey: false,
    toolApprovalMode: 'auto',
    agentMode: 'single',
    workspaceRoot: '',
    leadAgentId: 'general',
    contextCompressionEnabled: true,
    contextBudgetTokens: DEFAULT_CONTEXT_BUDGET_TOKENS,
    contextKeepRecentUserTurns: 3,
    contextSummaryMaxTokens: 1024,
    maxToolRounds: DEFAULT_TOOL_ROUNDS,
    fileReadMaxBytes: 65_536,
    fileLineMaxBytes: 1024,
    fileGrepMaxResults: 50,
    terminalOutputMaxBytes: 16_384,
    terminalTimeoutSeconds: 30,
    terminalMaxWallHours: 24,
    attachmentUploadMaxBytes: 100 * 1024 * 1024,
    maxSubAgentToolRounds: DEFAULT_SUB_AGENT_TOOL_ROUNDS,
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
    mediaModelOverrides: {},
    computerTierLlm: {},
    computerPipelineLlm: {
      decision: '',
      position: '',
      verify: '',
      decisionProviderId: '',
      positionProviderId: '',
      verifyProviderId: '',
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
    const fromTemplates = platformTemplateProviders(platformIn.platformProviders)
    const platformSource = platformIn.providers?.length
      ? platformIn.providers
      : fromTemplates
    const normalizedPlatformProviders = normalizeProviders(
      platformSource,
      undefined,
      globalGenFallbackFrom(mergedIn)
    )
    platformSettings.value = {
      ...defaultPlatformSettings(),
      ...platformIn,
      providers: normalizedPlatformProviders
    }
    canEditPlatform.value = view.canEditPlatform
    isPlatformAdmin.value = view.isPlatformAdmin
    const activeId = mergedIn.activeProviderId || ''
    const platformById = new Map(normalizedPlatformProviders.map(provider => [provider.id, provider]))
    const seenMerged = new Set<string>()
    const mergedProviders: ProviderConfig[] = (mergedIn.providers ?? []).map(provider => {
      seenMerged.add(provider.id)
      const plat = platformById.get(provider.id)
      if (!plat) return provider
      // source=user 自己管名称 / 地址 / 模型名单（桌面 fork 或 standalone 自定义服务）。
      // 平台目录仍拥有未 fork 的 platform 服务，刷新后能看到新模型。
      if (provider.source === 'user') {
        return {
          ...provider,
          source: 'user',
          modelConfigs: mergePlatformModelConfigs(plat.modelConfigs, provider.modelConfigs)
        }
      }
      return {
        ...provider,
        source: (provider.source ?? 'platform') as 'user' | 'platform',
        name: plat.name || provider.name,
        baseUrl: plat.baseUrl || provider.baseUrl,
        models: plat.models.length ? [...plat.models] : (provider.models ?? []),
        modelConfigs: mergePlatformModelConfigs(plat.modelConfigs, provider.modelConfigs)
      }
    })
    for (const plat of normalizedPlatformProviders) {
      if (seenMerged.has(plat.id)) continue
      mergedProviders.push({ ...plat, source: (plat.source ?? 'platform') as 'user' | 'platform' })
    }
    const nextMerged = {
      ...mergedIn,
      providers: mergedProviders,
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

  function storeGlobalGenFallbackFrom(st?: Pick<ModelSettings, 'temperature' | 'maxTokens' | 'contextBudgetTokens'>): RuntimeGenFallback {
    return {
      temperature: () => st?.temperature ?? settings.value.temperature,
      maxTokens: () => st?.maxTokens ?? settings.value.maxTokens,
      contextBudgetTokens: () => st?.contextBudgetTokens ?? settings.value.contextBudgetTokens
    }
  }

  const activeProvider = computed((): ProviderConfig | undefined => {
    const list = settings.value.providers
    if (!list.length) return undefined
    return list.find(p => p.id === settings.value.activeProviderId) ?? list[0]
  })

  const activeBaseUrl = computed(() => activeProvider.value?.baseUrl ?? '')
  const activeModelList = computed(() => activeProvider.value?.models ?? [])

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
    // Live tier maps live on the merged view. userSettings can still be the
    // last GET (empty maps for non-admins before this fix). Spreading only
    // userSettings would persist stale defaults and wipe a custom fast tier
    // when saving another field (e.g. switching the standard-tier model).
    return cloneJson({
      ...userSettings.value,
      agentModeLlm: settings.value.agentModeLlm ?? userSettings.value.agentModeLlm,
      mediaModeLlm: settings.value.mediaModeLlm ?? userSettings.value.mediaModeLlm,
      computerTierLlm: settings.value.computerTierLlm ?? userSettings.value.computerTierLlm,
      ...patch
    })
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
    // 后端 update_user_settings 会把空/掩码的 apiKey 从内存 key 池回填后再落盘，
    // 返回的 merged.providers 才是最新的 key 状态。若不把 providers 同步回来，
    // 本地 merged 视图里编辑项的 apiKey 停留在保存前被置空的值，UI 会显示
    // 「保存后 key 丢了」（后端其实已存好，重启后又恢复）。只刷新 merged 的
    // provider/模型相关字段，不动 platform map 与 session debug drafts。
    const mergedIn = view.merged ?? ({} as ModelSettings)
    settings.value = {
      ...settings.value,
      providers: normalizeProviders(
        mergedIn.providers,
        undefined,
        storeGlobalGenFallbackFrom(mergedIn)
      ),
      activeProviderId: mergedIn.activeProviderId ?? settings.value.activeProviderId,
      model: mergedIn.model ?? settings.value.model,
      temperature: mergedIn.temperature ?? settings.value.temperature,
      maxTokens: mergedIn.maxTokens ?? settings.value.maxTokens,
      fileReadMaxBytes: mergedIn.fileReadMaxBytes ?? user.fileReadMaxBytes ?? settings.value.fileReadMaxBytes,
      fileLineMaxBytes: mergedIn.fileLineMaxBytes ?? user.fileLineMaxBytes ?? settings.value.fileLineMaxBytes,
      fileGrepMaxResults:
        mergedIn.fileGrepMaxResults ?? user.fileGrepMaxResults ?? settings.value.fileGrepMaxResults,
      terminalOutputMaxBytes:
        mergedIn.terminalOutputMaxBytes
        ?? user.terminalOutputMaxBytes
        ?? settings.value.terminalOutputMaxBytes,
      terminalTimeoutSeconds:
        mergedIn.terminalTimeoutSeconds
        ?? user.terminalTimeoutSeconds
        ?? settings.value.terminalTimeoutSeconds,
      terminalMaxWallHours:
        mergedIn.terminalMaxWallHours
        ?? user.terminalMaxWallHours
        ?? settings.value.terminalMaxWallHours,
      attachmentUploadMaxBytes:
        mergedIn.attachmentUploadMaxBytes
        ?? user.attachmentUploadMaxBytes
        ?? settings.value.attachmentUploadMaxBytes,
      maxToolRounds: mergedIn.maxToolRounds ?? user.maxToolRounds ?? settings.value.maxToolRounds,
      maxSubAgentToolRounds: normalizeSubAgentToolRounds(
        mergedIn.maxSubAgentToolRounds
        ?? user.maxSubAgentToolRounds
        ?? settings.value.maxSubAgentToolRounds
      ),
      toolApprovalMode: mergedIn.toolApprovalMode ?? user.toolApprovalMode ?? settings.value.toolApprovalMode,
      computerHumanLike: mergedIn.computerHumanLike ?? user.computerHumanLike ?? settings.value.computerHumanLike,
      computerAutoSwitchMonitor:
        mergedIn.computerAutoSwitchMonitor
        ?? user.computerAutoSwitchMonitor
        ?? settings.value.computerAutoSwitchMonitor,
      captchaSliderOffsetPx:
        mergedIn.captchaSliderOffsetPx
        ?? user.captchaSliderOffsetPx
        ?? settings.value.captchaSliderOffsetPx,
      contextCompressionEnabled: true,
      contextBudgetTokens:
        mergedIn.contextBudgetTokens
        ?? user.contextBudgetTokens
        ?? settings.value.contextBudgetTokens,
      contextKeepRecentUserTurns:
        mergedIn.contextKeepRecentUserTurns
        ?? user.contextKeepRecentUserTurns
        ?? settings.value.contextKeepRecentUserTurns,
      // WEB 非 admin 会剥掉调试字段：省略时保留内存值，不能当成 false。
      taskBoardShowChildBoards:
        mergedIn.taskBoardShowChildBoards
        ?? user.taskBoardShowChildBoards
        ?? settings.value.taskBoardShowChildBoards,
      rawContentViewEnabled:
        mergedIn.rawContentViewEnabled
        ?? user.rawContentViewEnabled
        ?? settings.value.rawContentViewEnabled,
      computerAnnotatedScreenViewEnabled:
        mergedIn.computerAnnotatedScreenViewEnabled
        ?? user.computerAnnotatedScreenViewEnabled
        ?? settings.value.computerAnnotatedScreenViewEnabled,
      agentUiOverrides:
        mergedIn.agentUiOverrides
        ?? user.agentUiOverrides
        ?? settings.value.agentUiOverrides,
      mediaModelOverrides:
        mergedIn.mediaModelOverrides
        ?? user.mediaModelOverrides
        ?? settings.value.mediaModelOverrides,
      // 并发项必须写回 merged：设置弹窗关闭再开会从 settings 回填。
      // Option 字段省略表示「跟 CPU」，不能用旧 merged 数字兜底，否则改回自动后界面仍显示上次的值。
      parallelToolExecutionEnabled:
        mergedIn.parallelToolExecutionEnabled
        ?? user.parallelToolExecutionEnabled
        ?? settings.value.parallelToolExecutionEnabled
        ?? true,
      maxParallelToolCalls: mergedIn.maxParallelToolCalls ?? user.maxParallelToolCalls ?? null,
      maxParallelSubAgents: mergedIn.maxParallelSubAgents ?? user.maxParallelSubAgents ?? null,
      maxParallelMediaJobs: mergedIn.maxParallelMediaJobs ?? user.maxParallelMediaJobs ?? null,
      maxConcurrentRuns:
        mergedIn.maxConcurrentRuns ?? user.maxConcurrentRuns ?? settings.value.maxConcurrentRuns ?? 4,
      hasKey: mergedIn.hasKey ?? settings.value.hasKey,
      theme: user.theme,
      // 场景档位是用户层配置，必须写回 merged，否则输入框/下一轮仍读旧值。
      computerInitialTier: normalizeComputerInitialTier(
        mergedIn.computerInitialTier ?? user.computerInitialTier ?? settings.value.computerInitialTier
      ),
      agentPerformanceModes: {
        ...(settings.value.agentPerformanceModes ?? {}),
        ...(user.agentPerformanceModes ?? {}),
        ...(mergedIn.agentPerformanceModes ?? {})
      },
      mediaUnderstandingModes: {
        ...(settings.value.mediaUnderstandingModes ?? {}),
        ...(user.mediaUnderstandingModes ?? {}),
        ...(mergedIn.mediaUnderstandingModes ?? {})
      },
      agentModeLlm: mergedIn.agentModeLlm ?? user.agentModeLlm ?? settings.value.agentModeLlm,
      mediaModeLlm: mergedIn.mediaModeLlm ?? user.mediaModeLlm ?? settings.value.mediaModeLlm,
      computerTierLlm: mergedIn.computerTierLlm ?? user.computerTierLlm ?? settings.value.computerTierLlm,
      computerPipelineLlm:
        mergedIn.computerPipelineLlm ?? user.computerPipelineLlm ?? settings.value.computerPipelineLlm
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

  async function saveUserPreferencesSnapshot(snapshot: UserSettings) {
    // 用户板块统一走 update_user_settings：前端只发「当前 user 层 + 板块 patch」
    // 的 UserSettings 快照，后端直接覆盖落盘，无需 merge。
    await saveUserSnapshot(snapshot)
  }

  async function saveSession(patch: Partial<UserSettings>) {
    if (patch.theme !== undefined) {
      applyTheme(patch.theme)
      settings.value.theme = patch.theme
    }
    if (Object.keys(patch).length === 0) return
    await saveUserPreferencesSnapshot(createUserSnapshot(patch))
  }

  async function saveAgentPreferences(patch: Partial<UserSettings>) {
    await saveUserPreferencesSnapshot(createUserSnapshot(patch))
  }

  async function saveModelService(patch: Partial<UserSettings>) {
    // 模型配置统一走 update_user_settings（唯一用户持久化端点）；
    // providers/activeProviderId/model/temperature/maxTokens 以板块快照落盘。
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

  async function save(patch: Partial<UserSettings>) {
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
    settings.value = { ...settings.value, mediaModelOverrides: next }
    userSettings.value = { ...userSettings.value, mediaModelOverrides: next }
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
    saveSession,
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
