import { computed, getCurrentInstance, inject, onScopeDispose, provide, ref, watch, type InjectionKey, type Ref } from 'vue'
import {
  Bot,
  CircleHelp,
  Cpu,
  Database,
  Gauge,
  Info,
  Monitor,
  Moon,
  Network,
  Sparkles,
  Sun,
  UserCircle,
  Users,
  Wrench
} from 'lucide-vue-next'
import type {
  AgentDef,
  AgentUiConfig,
  ComputerInitialTier,
  ComputerPipelineLlmSettings,
  ComputerTierKey,
  ComputerTierLlmConfig,
  MediaModelOverrides,
  PerformanceMode,
  PerformanceModeKey,
  ThemePreference
} from '../types/chat'
import { COMPUTER_INITIAL_TIER_OPTIONS, PERFORMANCE_MODE_OPTIONS } from '../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../types/chat'
import { applyTheme } from '../lib/theme'
import { randomUuid } from '../lib/randomUuid'
import { resolveAgentUi, composerAgentLabel } from '../lib/agentUi'
import { sortComposerAgents, TEAM_MODE_UI_ENABLED } from '../lib/agentIcons'
import { listAgents, checkMediaDeps } from '../lib/api'
import { usePlatformAuthStore } from '../stores/platformAuth'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'

export type SettingsDialogForm = ReturnType<typeof createSettingsDialogForm>

export const SettingsDialogFormKey: InjectionKey<SettingsDialogForm> = Symbol.for('SettingsDialogForm')

let activeSettingsDialogForm: SettingsDialogForm | null = null

export function provideSettingsDialogForm(deps: {
  onClose: () => void
  activeSection: Ref<string>
  debugSectionIds: ReadonlySet<string>
}) {
  const form = createSettingsDialogForm(deps)
  activeSettingsDialogForm = form
  provide(SettingsDialogFormKey, form)
  if (getCurrentInstance()) {
    onScopeDispose(() => {
      if (activeSettingsDialogForm === form) {
        activeSettingsDialogForm = null
      }
    })
  }
  return form
}

export function useSettingsDialogForm() {
  const form = inject(SettingsDialogFormKey, null) ?? activeSettingsDialogForm
  if (!form) {
    throw new Error('useSettingsDialogForm() must be used inside SettingsDialog')
  }
  return form
}

function createSettingsDialogForm(deps: {
  onClose: () => void
  activeSection: Ref<string>
  debugSectionIds: ReadonlySet<string>
  }) {
  const { onClose, activeSection, debugSectionIds } = deps

  const s = useSettingsStore()
  const platformAuth = usePlatformAuthStore()
  const chat = useChatStore()

  const platformReadOnly = computed(() => !s.canEditPlatform)

  const COMPUTER_TIER_UI: { key: ComputerTierKey; label: string }[] = [
  { key: 'primary', label: '快速' },
  { key: 'intermediate', label: '标准' },
  { key: 'advanced', label: '专家' }
  ]

  const PERFORMANCE_MODE_UI = PERFORMANCE_MODE_OPTIONS

  const PERFORMANCE_MODE_HELP =
  '快速、标准、专家由低到高：速度从高到低，价格从低到高，智能从低到高。'

  const MEDIA_DEBUG_KINDS = ['image', 'audio', 'video'] as const
  type MediaDebugKind = (typeof MEDIA_DEBUG_KINDS)[number]

  const MODE_AGENT_IDS = new Set(['general', 'coder'])

  const AGENT_MODE_USER_ROWS: { id: string; label: string }[] = [
  { id: 'general', label: '通用助手' },
  { id: 'coder', label: '氛围编程' }
  ]

  const MEDIA_MODE_USER_ROWS: { key: MediaDebugKind; label: string }[] = [
  { key: 'image', label: '图片理解' },
  { key: 'audio', label: '语音转写' },
  { key: 'video', label: '视频理解' }
  ]

  function isModeAgent(agentId: string): boolean {
  return MODE_AGENT_IDS.has(agentId)
  }

  function patchAgentModeLlm(agentId: string, mode: PerformanceModeKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.platformSettings.agentModeLlm ?? {}) }
  const agentMap = { ...(next[agentId] ?? {}) }
  const prev = agentMap[mode] ?? agentModeLlm(agentId, mode)
  agentMap[mode] = { ...prev, ...patch }
  next[agentId] = agentMap
  s.platformSettings.agentModeLlm = next
  }

  function agentModeLlm(agentId: string, mode: PerformanceModeKey): ComputerTierLlmConfig {
  const m = s.platformSettings.agentModeLlm?.[agentId]?.[mode]
  if (m) return m
  if (mode === 'fast') {
    return { providerId: 'deepseek', model: 'deepseek-v4-flash', enableThinking: true, thinkingBudget: 2048 }
  }
  if (mode === 'expert') {
    const expertModel = agentId === 'general' ? 'qwen3.7-plus' : 'qwen3.7-max'
    return { providerId: 'qwen', model: expertModel, enableThinking: true, thinkingBudget: 8192 }
  }
  return { providerId: 'deepseek', model: 'deepseek-v4-pro', enableThinking: true, thinkingBudget: 2048 }
  }

  function patchMediaModeLlm(kind: MediaDebugKind, mode: PerformanceModeKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.platformSettings.mediaModeLlm ?? {}) }
  const kindMap = { ...(next[kind] ?? {}) }
  const prev = kindMap[mode] ?? {
    providerId: 'qwen',
    model: mode === 'fast' ? 'qwen3.5-flash' : mode === 'expert' ? 'qwen3.6-plus' : 'qwen3.5-plus',
    enableThinking: true,
    thinkingBudget: mode === 'expert' ? 8192 : 2048
  }
  kindMap[mode] = { ...prev, ...patch }
  next[kind] = kindMap
  s.platformSettings.mediaModeLlm = next
  }

  function mediaModeLlm(kind: MediaDebugKind, mode: PerformanceModeKey): ComputerTierLlmConfig {
  const m = s.platformSettings.mediaModeLlm?.[kind]?.[mode]
  if (m) return m
  if (kind === 'audio') {
    return {
      providerId: 'qwen',
      model: mode === 'fast' ? 'qwen3-asr-flash' : 'fun-asr',
      enableThinking: true,
      thinkingBudget: mode === 'expert' ? 8192 : 2048
    }
  }
  return {
    providerId: 'qwen',
    model: mode === 'fast' ? 'qwen3.5-flash' : mode === 'expert' ? 'qwen3.6-plus' : 'qwen3.5-plus',
    enableThinking: true,
    thinkingBudget: mode === 'expert' ? 8192 : 2048
  }
  }

  function computerTierLlm(key: ComputerTierKey): ComputerTierLlmConfig {
  const m = s.platformSettings.computerTierLlm?.[key]
  return (
    m ?? {
      providerId: 'qwen',
      model: key === 'advanced' ? 'qwen3.7-plus' : 'qwen3.5-plus',
      enableThinking: true,
      thinkingBudget: key === 'advanced' ? 8192 : 2048
    }
  )
  }

  function patchComputerTierLlm(key: ComputerTierKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.platformSettings.computerTierLlm ?? {}) }
  next[key] = { ...computerTierLlm(key), ...patch }
  s.platformSettings.computerTierLlm = next
  }

  function computerTierModelValue(key: ComputerTierKey): string {
  const c = computerTierLlm(key)
  return `${c.providerId}:${c.model}`
  }

  function selectComputerTierModel(key: ComputerTierKey, value: string) {
  const i = value.indexOf(':')
  if (i > 0 && i < value.length - 1) {
    const providerId = value.slice(0, i).trim()
    const model = value.slice(i + 1).trim()
    if (providerId && model) {
      patchComputerTierLlm(key, { providerId, model })
      return
    }
  }
  console.warn('[settings] selectComputerTierModel: invalid value', value)
  }

  function computerPipelineLlm(): ComputerPipelineLlmSettings {
  const defaults = {
    decision: 'qwen3.5-flash',
    position: 'qwen3.5-plus',
    verify: 'qwen3.5-flash',
    decisionProviderId: 'qwen',
    positionProviderId: 'qwen',
    verifyProviderId: 'qwen',
    positionThinkingBudget: 1024,
    verifyThinkingBudget: 256
  }
  return { ...defaults, ...s.platformSettings.computerPipelineLlm }
  }

  function patchComputerPipelineLlm(patch: Partial<ComputerPipelineLlmSettings>) {
  s.platformSettings.computerPipelineLlm = { ...computerPipelineLlm(), ...patch }
  }

  function computerPipelineVerifyValue(): string {
  const p = computerPipelineLlm()
  return `${p.verifyProviderId ?? 'qwen'}:${p.verify ?? 'qwen3.5-flash'}`
  }

  function selectComputerPipelineVerify(value: string) {
  const i = value.indexOf(':')
  if (i > 0 && i < value.length - 1) {
    const verifyProviderId = value.slice(0, i).trim()
    const verify = value.slice(i + 1).trim()
    if (verifyProviderId && verify) {
      patchComputerPipelineLlm({ verifyProviderId, verify })
      return
    }
  }
  console.warn('[settings] selectComputerPipelineVerify: invalid value', value)
  }

  const toolApprovalMode = ref<'auto' | 'manual'>('auto')
  const agentMode = ref<'single' | 'supervisor'>('single')
  const leadAgentId = ref('')
  const contextCompressionEnabled = ref(true)
  const contextBudgetTokens = ref(120_000)
  const contextKeepRecentUserTurns = ref(6)
  const maxToolRounds = ref(100)
  const maxSubAgentToolRounds = ref(100)
  const parallelToolExecutionEnabled = ref(true)
  const maxParallelToolCalls = ref<number | ''>('')
  const maxParallelSubAgents = ref<number | ''>('')
  const maxParallelMediaJobs = ref<number | ''>('')
  const maxConcurrentRuns = ref(4)
  const maxSubAgentSpawnDepth = ref(2)
  const rawContentViewEnabled = ref(false)
  const debugDumpLlmPrompts = ref(false)
  const terminalEnvRows = ref<Array<{ id: string; key: string; value: string }>>([])
  const taskBoardShowChildBoards = ref(false)
  const agentTaskBoardHistoryTrim = ref<Record<string, boolean>>({})
  const computerHumanLike = ref(false)
  const computerAutoSwitchMonitor = ref(true)
  const computerAutoCompact = ref(true)
  const collapseProcessByDefault = ref(false)
  const userCodingRules = ref('')
  const computerInitialTier = ref<ComputerInitialTier>('intermediate')
  const computerAnnotatedScreenViewEnabled = ref(false)
  const captchaSliderOffsetPx = ref(0)
  const theme = ref<ThemePreference>('system')
  const debugMenusEnabled = ref(false)
  const agentUiLocal = ref<Partial<AgentUiConfig>>({})
  const mediaImageGenerationModel = ref('')
  const mediaVideoGenerationModel = ref('')

  const agentPerformanceModesLocal = ref<Record<string, PerformanceMode>>({})
  const mediaUnderstandingModesLocal = ref<{ image: PerformanceMode; audio: PerformanceMode; video: PerformanceMode }>({
  image: 'fast',
  audio: 'fast',
  video: 'fast'
  })

  const mediaDeps = ref<import('../types/chat').MediaDepsStatus | null>(null)
  const agents = ref<AgentDef[]>([])

  const TOOL_CALL_UI_FIELDS: { key: keyof AgentUiConfig; label: string }[] = [
  { key: 'showSidecarToolCalls', label: '显示 sidecar 工具调用' },
  { key: 'showNonSidecarToolCalls', label: '显示非 sidecar 工具调用' },
  { key: 'showToolCalls', label: '显示工具调用卡片' },
  { key: 'showToolCallResults', label: '显示工具调用结果' },
  ]

  const AGENT_OUTPUT_UI_FIELDS: { key: keyof AgentUiConfig; label: string }[] = [
  { key: 'showReasoning', label: '显示推理过程' },
  { key: 'showTaskBoardPanel', label: '显示任务板面板' },
  { key: 'showSubAgentTrace', label: '显示子 Agent 边框面板' },
  ]

  const showDebugMenus = computed(() => debugMenusEnabled.value)

  const platformAccountTitle = computed(() => {
  if (!platformAuth.session.logged_in) return '未登录'
  return platformAuth.session.user_nickname?.trim() || '已登录'
  })

  const platformLogoutBusy = ref(false)

  async function logoutPlatformAccount() {
  platformLogoutBusy.value = true
  try {
    await platformAuth.logout()
  } catch (e) {
    console.error('[settings] platform logout failed', e)
  } finally {
    platformLogoutBusy.value = false
  }
  }

  async function loginPlatformAccount() {
  try {
    await platformAuth.login()
    chat.clearPlatformLoginErrorMessages()
  } catch (e) {
    console.error('[settings] platform login failed', e)
  }
  }

  const enabledWorkers = computed(() =>
  sortComposerAgents(agents.value.filter(a => a.role === 'worker' && a.enabled))
  )

  /** Lead agents the user may pick in the chat composer (settings list shows all enabled workers). */
  const selectableWorkers = computed(() =>
  enabledWorkers.value.filter(a => resolveAgentUi(a, s.settings).userSelectable)
  )

  function isLeadAgentSelectable(agent: AgentDef): boolean {
  return resolveAgentUi(agent, s.settings).userSelectable
  }

  function selectLeadWorker(agent: AgentDef) {
  if (!isLeadAgentSelectable(agent)) return
  agentMode.value = 'single'
  leadAgentId.value = agent.id
  }

  const supervisorAgent = computed(
  () =>
    agents.value.find(a => a.id === 'supervisor' && a.enabled) ||
    agents.value.find(a => a.role === 'supervisor')
  )

  const activeUiAgentId = computed(() =>
  agentMode.value === 'supervisor' ? 'supervisor' : (leadAgentId.value?.trim() || DEFAULT_LEAD_AGENT_ID)
  )

  const activeUiAgentLabel = computed(() => {
  const id = activeUiAgentId.value
  if (id === 'supervisor') return composerAgentLabel(supervisorAgent.value, s.settings)
  const agent = selectableWorkers.value.find(w => w.id === id) ?? selectableWorkers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return composerAgentLabel(agent, s.settings)
  })

  const effectiveDisplayUi = computed(() => {
  const id = activeUiAgentId.value
  const agent =
    agentMode.value === 'supervisor'
      ? supervisorAgent.value
      : selectableWorkers.value.find(w => w.id === id) ?? selectableWorkers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return resolveAgentUi(agent, {
    agentUiOverrides: {
      ...(s.settings.agentUiOverrides ?? {}),
      [id]: agentUiLocal.value
    }
  })
  })

  function isLeadWorkerSelected(agentId: string): boolean {
  if (agentMode.value !== 'single') return false
  const id = leadAgentId.value?.trim() || DEFAULT_LEAD_AGENT_ID
  return id === agentId
  }

  function displayUiChecked(key: keyof AgentUiConfig): boolean {
  const map: Record<string, boolean> = {
    showSidecarToolCalls: effectiveDisplayUi.value.showSidecarToolCalls,
    showNonSidecarToolCalls: effectiveDisplayUi.value.showNonSidecarToolCalls,
    showReasoning: effectiveDisplayUi.value.showReasoning,
    showSubAgentTrace: effectiveDisplayUi.value.showSubAgentTrace,
    showToolCalls: effectiveDisplayUi.value.showToolCalls,
    showToolCallResults: effectiveDisplayUi.value.showToolCallResults,
    showTaskBoardPanel: effectiveDisplayUi.value.showTaskBoardPanel,
  }
  return map[key as string] ?? true
  }

  function setDisplayUi(key: keyof AgentUiConfig, checked: boolean) {
  agentUiLocal.value = { ...agentUiLocal.value, [key]: checked }
  s.patchAgentUiOverride(activeUiAgentId.value, { [key]: checked })
  }

  async function applyThemeChoice(t: ThemePreference) {
  theme.value = t
  applyTheme(t)
  // Theme persists in UserSettings; keep both mirrors in sync so a later
  // applyEffectiveView (from unrelated saves) does not resurrect the old value
  // before saveUser({ theme }) runs.
  s.settings.theme = t
  s.userSettings.theme = t
  }

  function themeLabel(t: ThemePreference): string {
  if (t === 'light') return '浅色'
  if (t === 'dark') return '深色'
  return '跟随系统'
  }

  function nextTheme(t: ThemePreference): ThemePreference {
  if (t === 'system') return 'light'
  if (t === 'light') return 'dark'
  return 'system'
  }

  async function cycleTheme() {
  await applyThemeChoice(nextTheme(theme.value))
  }

  const currentThemeIcon = computed(() => {
  if (theme.value === 'light') return Sun
  if (theme.value === 'dark') return Moon
  return Monitor
  })

  async function loadAgents() {
  try {
    agents.value = await listAgents()
  } catch (e) {
    console.error(e)
  }
  }

  function initFormFromStore() {
  toolApprovalMode.value = s.settings.toolApprovalMode || 'auto'
  agentMode.value =
    !TEAM_MODE_UI_ENABLED && s.settings.agentMode === 'supervisor'
      ? 'single'
      : (s.settings.agentMode || 'single')
  leadAgentId.value = s.settings.leadAgentId || DEFAULT_LEAD_AGENT_ID
  contextCompressionEnabled.value = s.settings.contextCompressionEnabled !== false
  contextBudgetTokens.value =
    s.settings.contextBudgetTokens ??
    (s.settings as { contextBudgetChars?: number }).contextBudgetChars ??
    120_000
  contextKeepRecentUserTurns.value = s.settings.contextKeepRecentUserTurns ?? 6
  maxToolRounds.value = s.settings.maxToolRounds ?? 100
  parallelToolExecutionEnabled.value = s.settings.parallelToolExecutionEnabled !== false
  maxParallelToolCalls.value = s.settings.maxParallelToolCalls ?? ''
  maxParallelSubAgents.value = s.settings.maxParallelSubAgents ?? ''
  maxParallelMediaJobs.value = s.settings.maxParallelMediaJobs ?? ''
  maxConcurrentRuns.value = s.settings.maxConcurrentRuns ?? 4
  maxSubAgentToolRounds.value = s.settings.maxSubAgentToolRounds ?? s.settings.maxToolRounds ?? 100
  maxSubAgentSpawnDepth.value = s.settings.maxSubAgentSpawnDepth ?? 2
  rawContentViewEnabled.value = s.settings.rawContentViewEnabled === true
  debugDumpLlmPrompts.value = s.settings.debugDumpLlmPrompts === true
  terminalEnvRows.value = Object.entries(s.settings.terminalEnvOverrides ?? {}).map(([key, value]) => ({
    id: randomUuid(),
    key,
    value
  }))
  taskBoardShowChildBoards.value = s.settings.taskBoardShowChildBoards === true
  agentTaskBoardHistoryTrim.value = { ...(s.settings.agentTaskBoardHistoryTrim ?? {}) }
  computerHumanLike.value = s.settings.computerHumanLike === true
  computerAutoSwitchMonitor.value = s.settings.computerAutoSwitchMonitor !== false
  computerAutoCompact.value = s.userSettings.computerAutoCompact !== false
  collapseProcessByDefault.value = s.userSettings.collapseProcessByDefault === true
  userCodingRules.value = s.userSettings.userCodingRules ?? ''
  computerInitialTier.value = s.settings.computerInitialTier ?? 'intermediate'
  computerAnnotatedScreenViewEnabled.value = s.settings.computerAnnotatedScreenViewEnabled === true
  captchaSliderOffsetPx.value = Number(s.settings.captchaSliderOffsetPx ?? 0) || 0
  theme.value = (s.settings.theme as ThemePreference) || 'system'
  debugMenusEnabled.value = s.canEditPlatform && s.settings.debugMenusEnabled === true
  agentUiLocal.value = { ...(s.settings.agentUiOverrides?.[activeUiAgentId.value] ?? {}) }
  mediaImageGenerationModel.value = getMediaModelWithProvider('imageGeneration')
  mediaVideoGenerationModel.value = getMediaModelWithProvider('videoGeneration')
  agentPerformanceModesLocal.value = {
    general: s.getAgentPerformanceMode('general'),
    coder: s.getAgentPerformanceMode('coder')
  }
  mediaUnderstandingModesLocal.value = {
    image: s.getMediaUnderstandingMode('image'),
    audio: s.getMediaUnderstandingMode('audio'),
    video: s.getMediaUnderstandingMode('video')
  }
  void loadAgents()
  // Defer ffmpeg probe so opening settings → IM 通道 stays responsive on Windows.
  window.setTimeout(() => {
    if (activeSection.value === 'assistant' && mediaDeps.value === null) {
      void refreshMediaDeps()
    }
  }, 400)
  }

  watch(activeSection, section => {
  if (section === 'assistant' && mediaDeps.value === null) {
    void refreshMediaDeps()
  }
  })

  async function refreshMediaDeps() {
  try {
    mediaDeps.value = await checkMediaDeps()
  } catch (e) {
    console.warn('[settings] checkMediaDeps failed', e)
    mediaDeps.value = null
  }
  }

  const ffmpegStatusLabel = computed(() => {
  const deps = mediaDeps.value
  if (!deps) return '检测中…'
  switch (deps.status) {
    case 'ready':
      return '已就绪（可执行抽帧）'
    case 'partial':
      return '未就绪：缺少部分组件'
    case 'not_executable':
      return '未就绪：已找到但无法执行'
    default:
      return '未检测到'
  }
  })

  const ffmpegStatusDetail = computed(() => {
  const deps = mediaDeps.value
  if (!deps?.detail?.trim()) {
    if (deps?.status === 'ready' && deps.ffmpegPath) {
      return deps.ffmpegPath
    }
    return ''
  }
  return deps.detail.trim()
  })

  const ffmpegNeedsInstall = computed(() => {
  const status = mediaDeps.value?.status
  return status === 'not_found' || status === 'partial' || status === 'not_executable'
  })

  async function askAssistantInstallFfmpeg() {
  onClose()
  await chat.sendUserMessage('帮我安装 ffmpeg')
  }

  watch(activeUiAgentId, id => {
  agentUiLocal.value = { ...(s.settings.agentUiOverrides?.[id] ?? {}) }
  })

  watch(showDebugMenus, enabled => {
  if (!enabled) {
    if (debugSectionIds.has(activeSection.value)) {
      activeSection.value = 'assistant'
    }
  }
  })

  function taskBoardTrimChecked(agentId: string): boolean {
  const v = agentTaskBoardHistoryTrim.value[agentId]
  if (v !== undefined) return v
  return s.defaultTaskBoardHistoryTrim(agentId)
  }

  function setTaskBoardTrimLocal(agentId: string, enabled: boolean) {
  agentTaskBoardHistoryTrim.value = {
    ...agentTaskBoardHistoryTrim.value,
    [agentId]: enabled
  }
  }

  function getMediaModelWithProvider(kind: keyof MediaModelOverrides): string {
  const ref = s.getMediaModelOverride(kind)
  if (!ref?.model) return ''
  return `${ref.providerId}:${ref.model}`
  }

  async function selectMediaModelWithProvider(
  kind: keyof MediaModelOverrides,
  value: string
  ) {
  if (!value) {
    await s.setMediaModelOverride(kind, null)
    if (kind === 'imageGeneration') mediaImageGenerationModel.value = ''
    if (kind === 'videoGeneration') mediaVideoGenerationModel.value = ''
    return
  }
  const i = value.indexOf(':')
  if (i > 0 && i < value.length - 1) {
    const providerId = value.slice(0, i).trim()
    const model = value.slice(i + 1).trim()
    if (providerId && model) {
      await s.setMediaModelOverride(kind, { providerId, model })
      if (kind === 'imageGeneration') mediaImageGenerationModel.value = value
      if (kind === 'videoGeneration') mediaVideoGenerationModel.value = value
      return
    }
  }
  await s.setMediaModelOverride(kind, {
    providerId: s.settings.activeProviderId,
    model: value.trim()
  })
  }

  function selectAgentModeModel(agentId: string, mode: PerformanceModeKey, model: string) {
  const item = s.allModels.find(m => m.model === model)
  patchAgentModeLlm(agentId, mode, {
    model,
    providerId: item?.providerId ?? agentModeLlm(agentId, mode).providerId
  })
  }

  function selectMediaModeModel(kind: MediaDebugKind, mode: PerformanceModeKey, model: string) {
  const item = s.allModels.find(m => m.model === model)
  patchMediaModeLlm(kind, mode, {
    model,
    providerId: item?.providerId ?? mediaModeLlm(kind, mode).providerId
  })
  }

  /** Get agent default model with provider prefix: "providerId:model" */
  function getAgentModelWithProvider(agentId: string): string {
  const ref = s.getAgentDefaultModelRef(agentId)
  if (!ref?.model) return ''
  return `${ref.providerId}:${ref.model}`
  }

  /** Select agent model with provider prefix: "providerId:model" */
  async function selectAgentModelWithProvider(agentId: string, value: string) {
  if (!value) {
    await s.setAgentDefaultModel(agentId, null)
    return
  }
  const i = value.indexOf(':')
  if (i > 0 && i < value.length - 1) {
    const providerId = value.slice(0, i).trim()
    const model = value.slice(i + 1).trim()
    if (providerId && model) {
      if (s.settings.activeProviderId !== providerId) {
        await s.setActiveProvider(providerId)
      }
      await s.setAgentDefaultModel(agentId, { providerId, model })
      return
    }
  }
  await s.setAgentDefaultModel(agentId, {
    providerId: s.settings.activeProviderId,
    model: value.trim()
  })
  }

  /** Cleared from every agent when debug mode is turned off (restore profile defaults). */
  const DEBUG_AGENT_UI_KEYS: (keyof AgentUiConfig)[] = [
  'showSidecarToolCalls',
  'showToolCallResults',
  'showReasoning'
  ]

  function stripDebugAgentUiOverrides(
  overrides: Record<string, Partial<AgentUiConfig>> | undefined
  ): Record<string, Partial<AgentUiConfig>> {
  if (!overrides) return {}
  const out: Record<string, Partial<AgentUiConfig>> = {}
  for (const [id, cfg] of Object.entries(overrides)) {
    const next = { ...cfg }
    for (const key of DEBUG_AGENT_UI_KEYS) {
      delete next[key]
    }
    if (Object.keys(next).length > 0) {
      out[id] = next
    }
  }
  return out
  }

  async function toggleDebugMenus() {
  const next = !debugMenusEnabled.value
  debugMenusEnabled.value = next
  rawContentViewEnabled.value = next
  computerAnnotatedScreenViewEnabled.value = next

  if (next) {
    agentUiLocal.value = {
      ...agentUiLocal.value,
      showReasoning: true,
      showSidecarToolCalls: true
    }
    s.patchAgentUiOverride(activeUiAgentId.value, {
      showReasoning: true,
      showSidecarToolCalls: true
    })
  } else {
    debugDumpLlmPrompts.value = false
    taskBoardShowChildBoards.value = false
    agentUiLocal.value = {
      ...agentUiLocal.value,
      showSidecarToolCalls: false,
      showToolCallResults: false,
      showReasoning: false
    }
    s.patchAgentUiOverride(activeUiAgentId.value, {
      showSidecarToolCalls: false,
      showToolCallResults: false,
      showReasoning: false
    })
  }

  const baseOverrides = s.settings.agentUiOverrides ?? {}
  const agentUiOverrides = next
    ? {
        ...baseOverrides,
        [activeUiAgentId.value]: {
          ...(baseOverrides[activeUiAgentId.value] ?? {}),
          ...agentUiLocal.value,
          showReasoning: true
        }
      }
    : stripDebugAgentUiOverrides(baseOverrides)

  // Keep terminalEnvOverrides across debug toggle; injection stays active either way.
  const terminalEnvOverrides = terminalEnvOverridesFromRows()
  s.settings.terminalEnvOverrides = terminalEnvOverrides

  await s.save({
    debugMenusEnabled: next,
    rawContentViewEnabled: next,
    computerAnnotatedScreenViewEnabled: next,
    debugDumpLlmPrompts: next ? debugDumpLlmPrompts.value : false,
    taskBoardShowChildBoards: next ? taskBoardShowChildBoards.value : false,
    terminalEnvOverrides,
    agentUiOverrides
  })
  }

  function terminalEnvOverridesFromRows(): Record<string, string> {
    const out: Record<string, string> = {}
    for (const row of terminalEnvRows.value) {
      const key = row.key.trim()
      if (!key) continue
      out[key] = row.value
    }
    return out
  }

  function addTerminalEnvRow() {
    terminalEnvRows.value = [
      ...terminalEnvRows.value,
      { id: randomUuid(), key: '', value: '' }
    ]
  }

  function removeTerminalEnvRow(id: string) {
    terminalEnvRows.value = terminalEnvRows.value.filter(row => row.id !== id)
  }

  function optionalParallelLimit(v: number | ''): number | null {
    if (v === '') return null
    const n = Number(v)
    if (!Number.isFinite(n) || n < 1) return null
    return Math.floor(n)
  }

  function getAssistantSavePayload() {
  return {
    computerAutoCompact: computerAutoCompact.value,
    collapseProcessByDefault: collapseProcessByDefault.value,
    userCodingRules: userCodingRules.value.trim(),
    toolApprovalMode: toolApprovalMode.value,
    computerHumanLike: computerHumanLike.value,
    computerAutoSwitchMonitor: computerAutoSwitchMonitor.value,
    computerInitialTier: computerInitialTier.value,
    captchaSliderOffsetPx: Number(captchaSliderOffsetPx.value) || 0,
    parallelToolExecutionEnabled: parallelToolExecutionEnabled.value,
    maxParallelToolCalls: parallelToolExecutionEnabled.value
      ? optionalParallelLimit(maxParallelToolCalls.value)
      : null,
    maxParallelSubAgents: parallelToolExecutionEnabled.value
      ? optionalParallelLimit(maxParallelSubAgents.value)
      : null,
    maxParallelMediaJobs: parallelToolExecutionEnabled.value
      ? optionalParallelLimit(maxParallelMediaJobs.value)
      : null,
    maxConcurrentRuns: Math.max(1, Math.min(64, Number(maxConcurrentRuns.value) || 4)),
    contextCompressionEnabled: contextCompressionEnabled.value,
    contextBudgetTokens: Number(contextBudgetTokens.value),
    contextKeepRecentUserTurns: Number(contextKeepRecentUserTurns.value),
    maxToolRounds: Number(maxToolRounds.value),
    agentPerformanceModes: { ...agentPerformanceModesLocal.value },
    mediaUnderstandingModes: { ...mediaUnderstandingModesLocal.value },
  }
  }

  function getDebugSessionSavePayload() {
  return s.createDebugSessionSnapshot()
  }

  function getDebugRuntimeSavePayload() {
  const terminalEnvOverrides = terminalEnvOverridesFromRows()
  // Optimistic local write so later applyEffectiveView (theme / debug-session)
  // preserves overrides when the response omits or defaults the field.
  s.settings.terminalEnvOverrides = terminalEnvOverrides
  return {
    agentMode: agentMode.value,
    leadAgentId: agentMode.value === 'supervisor' ? '' : leadAgentId.value,
    maxSubAgentToolRounds: Number(maxSubAgentToolRounds.value),
    maxSubAgentSpawnDepth: Number(maxSubAgentSpawnDepth.value),
    rawContentViewEnabled: rawContentViewEnabled.value,
    debugDumpLlmPrompts: debugDumpLlmPrompts.value,
    terminalEnvOverrides,
    debugMenusEnabled: debugMenusEnabled.value,
    taskBoardShowChildBoards: taskBoardShowChildBoards.value,
    computerAnnotatedScreenViewEnabled: computerAnnotatedScreenViewEnabled.value,
    agentTaskBoardHistoryTrim: { ...agentTaskBoardHistoryTrim.value },
    agentUiOverrides: {
      ...(s.settings.agentUiOverrides ?? {}),
      [activeUiAgentId.value]: { ...agentUiLocal.value }
    },
  }
  }

  return {
    s,
    platformAuth,
    chat,
    platformReadOnly,
    COMPUTER_TIER_UI,
    COMPUTER_INITIAL_TIER_OPTIONS,
    composerAgentLabel,
    PERFORMANCE_MODE_UI,
    PERFORMANCE_MODE_HELP,
    MEDIA_DEBUG_KINDS,
    AGENT_MODE_USER_ROWS,
    MEDIA_MODE_USER_ROWS,
    TOOL_CALL_UI_FIELDS,
    AGENT_OUTPUT_UI_FIELDS,
    toolApprovalMode,
    agentMode,
    leadAgentId,
    contextCompressionEnabled,
    contextBudgetTokens,
    contextKeepRecentUserTurns,
    maxToolRounds,
    parallelToolExecutionEnabled,
    maxParallelToolCalls,
    maxParallelSubAgents,
    maxParallelMediaJobs,
    maxConcurrentRuns,
    maxSubAgentToolRounds,
    maxSubAgentSpawnDepth,
    rawContentViewEnabled,
    debugDumpLlmPrompts,
    terminalEnvRows,
    addTerminalEnvRow,
    removeTerminalEnvRow,
    taskBoardShowChildBoards,
    agentTaskBoardHistoryTrim,
    computerHumanLike,
    computerAutoSwitchMonitor,
    computerAutoCompact,
    collapseProcessByDefault,
    userCodingRules,
    computerInitialTier,
    computerAnnotatedScreenViewEnabled,
    captchaSliderOffsetPx,
    theme,
    debugMenusEnabled,
    agentUiLocal,
    mediaImageGenerationModel,
    mediaVideoGenerationModel,
    agentPerformanceModesLocal,
    mediaUnderstandingModesLocal,
    mediaDeps,
    agents,
    enabledWorkers,
    selectableWorkers,
    supervisorAgent,
    activeUiAgentId,
    activeUiAgentLabel,
    effectiveDisplayUi,
    platformAccountTitle,
    platformLogoutBusy,
    showDebugMenus,
    TEAM_MODE_UI_ENABLED,
    isLeadAgentSelectable,
    selectLeadWorker,
    isLeadWorkerSelected,
    displayUiChecked,
    setDisplayUi,
    applyThemeChoice,
    themeLabel,
    cycleTheme,
    currentThemeIcon,
    refreshMediaDeps,
    ffmpegStatusLabel,
    ffmpegStatusDetail,
    ffmpegNeedsInstall,
    askAssistantInstallFfmpeg,
    taskBoardTrimChecked,
    setTaskBoardTrimLocal,
    selectMediaModelWithProvider,
    selectAgentModeModel,
    selectMediaModeModel,
    getAgentModelWithProvider,
    selectAgentModelWithProvider,
    agentModeLlm,
    patchAgentModeLlm,
    mediaModeLlm,
    patchMediaModeLlm,
    computerTierLlm,
    patchComputerTierLlm,
    computerTierModelValue,
    selectComputerTierModel,
    computerPipelineLlm,
    patchComputerPipelineLlm,
    computerPipelineVerifyValue,
    selectComputerPipelineVerify,
    isModeAgent,
    toggleDebugMenus,
    logoutPlatformAccount,
    loginPlatformAccount,
    initFormFromStore,
    getAssistantSavePayload,
    getDebugSessionSavePayload,
    getDebugRuntimeSavePayload,
  }
}
