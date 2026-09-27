import { computed, getCurrentInstance, inject, nextTick, onScopeDispose, provide, ref, watch, type InjectionKey, type Ref } from 'vue'
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
import { sortComposerAgents } from '../lib/agentIcons'
import { listAgents, checkMediaDeps } from '../lib/api'
import { usePlatformAuthStore } from '../stores/platformAuth'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'
import { splitProviderModelValue } from '../lib/modelSelectValue'
import {
  emptyTierConfig,
  platformAgentModeDefault,
  platformComputerPipelineDefault,
  platformComputerTierDefault,
  platformMediaModeDefault,
  withInheritedThinking
} from '../lib/platformTierDefaults'
import { mergeTierLlmPatch, thinkingPatchFromProviderModel } from '../lib/thinkingIntensity'

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
  { key: 'advanced', label: '高级' }
  ]

  const PERFORMANCE_MODE_UI = PERFORMANCE_MODE_OPTIONS

  const PERFORMANCE_MODE_HELP =
  '快速、标准、高级由低到高：速度从高到低，价格从低到高，智能从低到高。'

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

  let debugModelSaveTimer: number | undefined

  function scheduleDebugModelSave() {
  if (debugModelSaveTimer) window.clearTimeout(debugModelSaveTimer)
  debugModelSaveTimer = window.setTimeout(() => {
    debugModelSaveTimer = undefined
    // 档位/管道模型映射统一走 agent-settings 持久化，重启保留。
    void s
      .saveAgentPreferences({
        computerTierLlm: s.settings.computerTierLlm,
        computerPipelineLlm: s.settings.computerPipelineLlm,
        agentModeLlm: s.settings.agentModeLlm,
        mediaModeLlm: s.settings.mediaModeLlm
      })
      .catch(error => {
        console.error('[settings] failed to save debug model mapping', error)
      })
  }, 250)
  }

  function patchAgentModeLlm(agentId: string, mode: PerformanceModeKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.settings.agentModeLlm ?? {}) }
  const agentMap = { ...(next[agentId] ?? {}) }
  const prev = agentMap[mode] ?? agentModeLlm(agentId, mode)
  agentMap[mode] = mergeTierLlmPatch(prev, patch)
  next[agentId] = agentMap
  s.settings.agentModeLlm = next
  scheduleDebugModelSave()
  }

  function agentModeLlm(agentId: string, mode: PerformanceModeKey): ComputerTierLlmConfig {
  const m = s.settings.agentModeLlm?.[agentId]?.[mode]
  if (m?.providerId && m.model) return m
  const fallback = platformAgentModeDefault(s.platformSettings.tierDefaults, agentId, mode)
  return fallback ? withInheritedThinking(fallback, s.settings.providers) : emptyTierConfig()
  }

  function patchMediaModeLlm(kind: MediaDebugKind, mode: PerformanceModeKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.settings.mediaModeLlm ?? {}) }
  const kindMap = { ...(next[kind] ?? {}) }
  const prev = kindMap[mode] ?? mediaModeLlm(kind, mode)
  kindMap[mode] = mergeTierLlmPatch(prev, patch)
  next[kind] = kindMap
  s.settings.mediaModeLlm = next
  scheduleDebugModelSave()
  }

  function mediaModeLlm(kind: MediaDebugKind, mode: PerformanceModeKey): ComputerTierLlmConfig {
  const m = s.settings.mediaModeLlm?.[kind]?.[mode]
  if (m?.providerId && m.model) return m
  const fallback = platformMediaModeDefault(s.platformSettings.tierDefaults, kind, mode)
  return fallback ? withInheritedThinking(fallback, s.settings.providers) : emptyTierConfig()
  }

  function computerTierLlm(key: ComputerTierKey): ComputerTierLlmConfig {
  const m = s.settings.computerTierLlm?.[key]
  if (m?.providerId && m.model) return m
  const fallback = platformComputerTierDefault(s.platformSettings.tierDefaults, key)
  return fallback ? withInheritedThinking(fallback, s.settings.providers) : emptyTierConfig()
  }

  function patchComputerTierLlm(key: ComputerTierKey, patch: Partial<ComputerTierLlmConfig>) {
  const next = { ...(s.settings.computerTierLlm ?? {}) }
  next[key] = mergeTierLlmPatch(computerTierLlm(key), patch)
  s.settings.computerTierLlm = next
  scheduleDebugModelSave()
  }

  function computerTierModelValue(key: ComputerTierKey): string {
  const c = computerTierLlm(key)
  return `${c.providerId}:${c.model}`
  }

  function selectComputerTierModel(key: ComputerTierKey, value: string) {
    const parsed = splitProviderModelValue(value)
    if (parsed) {
      const next = { ...(s.settings.computerTierLlm ?? {}) }
      next[key] = {
        providerId: parsed.providerId,
        model: parsed.model,
        ...thinkingPatchFromProviderModel(s.settings.providers, parsed.providerId, parsed.model)
      }
      s.settings.computerTierLlm = next
      scheduleDebugModelSave()
      return
    }
    console.warn('[settings] selectComputerTierModel: invalid value', value)
  }

  function computerPipelineLlm(): ComputerPipelineLlmSettings {
  const platformPipe = platformComputerPipelineDefault(s.platformSettings.tierDefaults)
  const current = s.settings.computerPipelineLlm ?? {}
  return {
    decision: current.decision || platformPipe.decision || '',
    position: current.position || platformPipe.position || '',
    verify: current.verify || platformPipe.verify || '',
    decisionProviderId: current.decisionProviderId || platformPipe.decisionProviderId || '',
    positionProviderId: current.positionProviderId || platformPipe.positionProviderId || '',
    verifyProviderId: current.verifyProviderId || platformPipe.verifyProviderId || '',
    positionThinkingBudget: current.positionThinkingBudget ?? 1024,
    verifyThinkingBudget: current.verifyThinkingBudget ?? 256
  }
  }

  function patchComputerPipelineLlm(patch: Partial<ComputerPipelineLlmSettings>) {
  s.settings.computerPipelineLlm = { ...computerPipelineLlm(), ...patch }
  scheduleDebugModelSave()
  }

  function computerPipelineVerifyValue(): string {
  const p = computerPipelineLlm()
  return p.verifyProviderId && p.verify ? `${p.verifyProviderId}:${p.verify}` : ''
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
  const agentMode = ref<'single'>('single')
  const leadAgentId = ref('')
  const contextKeepRecentUserTurns = ref(6)
  const DEFAULT_TOOL_ROUNDS = 5000
  const DEFAULT_SUB_AGENT_TOOL_ROUNDS = 500
  const CEILING_SUB_AGENT_TOOL_ROUNDS = 500
  const LEGACY_SUB_AGENT_TOOL_ROUNDS = new Set([200])
  const LEGACY_TOOL_ROUNDS = new Set([100, 200])
  function migrateToolRounds(raw?: number): number {
    const n = Number(raw)
    if (!Number.isFinite(n) || n < 1) return DEFAULT_TOOL_ROUNDS
    const rounds = Math.floor(n)
    if (LEGACY_TOOL_ROUNDS.has(rounds)) return DEFAULT_TOOL_ROUNDS
    return rounds
  }
  function migrateSubAgentToolRounds(raw?: number): number {
    const n = Number(raw)
    if (!Number.isFinite(n) || n < 1) return DEFAULT_SUB_AGENT_TOOL_ROUNDS
    const rounds = Math.floor(n)
    if (LEGACY_SUB_AGENT_TOOL_ROUNDS.has(rounds)) return DEFAULT_SUB_AGENT_TOOL_ROUNDS
    return Math.min(rounds, CEILING_SUB_AGENT_TOOL_ROUNDS)
  }
  const maxToolRounds = ref(DEFAULT_TOOL_ROUNDS)
  const fileReadMaxKb = ref(64)
  const fileLineMaxBytes = ref(1024)
  const fileGrepMaxResults = ref(50)
  const terminalOutputMaxKb = ref(16)
  const terminalTimeoutSeconds = ref(30)
  const terminalMaxWallHours = ref(24)
  const attachmentUploadMaxMb = ref(100)
  const maxSubAgentToolRounds = ref(DEFAULT_SUB_AGENT_TOOL_ROUNDS)
  const parallelToolExecutionEnabled = ref(true)
  const PARALLEL_LIMIT_CAP = 8
  function defaultParallelLimit(): number {
    const cores = Number(globalThis.navigator?.hardwareConcurrency)
    const n = Number.isFinite(cores) && cores >= 1 ? Math.floor(cores) : 1
    return Math.min(PARALLEL_LIMIT_CAP, Math.max(1, n))
  }
  const autoParallelLimit = defaultParallelLimit()
  const maxParallelToolCalls = ref(autoParallelLimit)
  const maxParallelSubAgents = ref(autoParallelLimit)
  const maxParallelMediaJobs = ref(autoParallelLimit)
  const maxConcurrentRuns = ref(4)
  function storedParallelLimit(v: number | null | undefined): number {
    const n = Number(v)
    return Number.isFinite(n) && n >= 1 ? Math.floor(n) : autoParallelLimit
  }
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
  { key: 'showTaskBoardPanel', label: '显示任务板' },
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
  leadAgentId.value = agent.id
  }

  const activeUiAgentId = computed(() =>
  leadAgentId.value?.trim() || DEFAULT_LEAD_AGENT_ID
  )

  const activeUiAgentLabel = computed(() => {
  const id = activeUiAgentId.value
  const agent = selectableWorkers.value.find(w => w.id === id) ?? selectableWorkers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return composerAgentLabel(agent, s.settings)
  })

  const effectiveDisplayUi = computed(() => {
  const id = activeUiAgentId.value
  const agent =
    selectableWorkers.value.find(w => w.id === id) ?? selectableWorkers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return resolveAgentUi(agent, {
    agentUiOverrides: {
      ...(s.settings.agentUiOverrides ?? {}),
      [id]: agentUiLocal.value
    }
  })
  })

  function isLeadWorkerSelected(agentId: string): boolean {
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
  scheduleAssistantSave()
  }

  async function applyThemeChoice(t: ThemePreference) {
  theme.value = t
  applyTheme(t)
  // Keep both mirrors in sync before persistence so concurrent settings updates
  // cannot temporarily restore the previous theme.
  s.settings.theme = t
  s.userSettings.theme = t
  try {
    await s.saveUser({ theme: t })
  } catch (e) {
    console.error('[settings] failed to save theme', e)
  }
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
  agentMode.value = 'single'
  leadAgentId.value = s.settings.leadAgentId || DEFAULT_LEAD_AGENT_ID
  contextKeepRecentUserTurns.value = s.settings.contextKeepRecentUserTurns ?? 6
  maxToolRounds.value = migrateToolRounds(s.settings.maxToolRounds)
  fileReadMaxKb.value = Math.max(4, Math.round((s.settings.fileReadMaxBytes ?? 65_536) / 1024))
  fileLineMaxBytes.value = s.settings.fileLineMaxBytes ?? 1024
  fileGrepMaxResults.value = s.settings.fileGrepMaxResults ?? 50
  terminalOutputMaxKb.value = Math.max(
    4,
    Math.round((s.settings.terminalOutputMaxBytes ?? 16_384) / 1024)
  )
  terminalTimeoutSeconds.value = Math.min(
    86_400,
    Math.max(1, Math.round(Number(s.settings.terminalTimeoutSeconds) || 30))
  )
  terminalMaxWallHours.value = Math.min(
    10_000,
    Math.max(1, Math.round(Number(s.settings.terminalMaxWallHours) || 24))
  )
  attachmentUploadMaxMb.value = Math.max(
    1,
    Math.round((s.settings.attachmentUploadMaxBytes ?? 100 * 1024 * 1024) / (1024 * 1024))
  )
  parallelToolExecutionEnabled.value = s.settings.parallelToolExecutionEnabled !== false
  maxParallelToolCalls.value = storedParallelLimit(s.settings.maxParallelToolCalls)
  maxParallelSubAgents.value = storedParallelLimit(s.settings.maxParallelSubAgents)
  maxParallelMediaJobs.value = storedParallelLimit(s.settings.maxParallelMediaJobs)
  maxConcurrentRuns.value = s.settings.maxConcurrentRuns ?? 4
  maxSubAgentToolRounds.value = migrateSubAgentToolRounds(s.settings.maxSubAgentToolRounds)
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
  theme.value = (s.settings.theme as ThemePreference) || 'system'
  debugMenusEnabled.value = s.canEditPlatform && s.settings.debugMenusEnabled === true
  agentUiLocal.value = { ...(s.settings.agentUiOverrides?.[activeUiAgentId.value] ?? {}) }
  mediaImageGenerationModel.value = getMediaModelWithProvider('imageGeneration')
  mediaVideoGenerationModel.value = getMediaModelWithProvider('videoGeneration')
  agentPerformanceModesLocal.value = {
    ...(s.settings.agentPerformanceModes ?? {}),
    general: s.getAgentPerformanceMode('general'),
    coder: s.getAgentPerformanceMode('coder')
  }
  mediaUnderstandingModesLocal.value = {
    image: s.getMediaUnderstandingMode('image'),
    audio: s.getMediaUnderstandingMode('audio'),
    video: s.getMediaUnderstandingMode('video')
  }
  void nextTick(() => {
    autosaveReady = true
  })
  void loadAgents()
  // Defer ffmpeg probe so opening settings → IM 通道 stays responsive on Windows.
  window.setTimeout(() => {
    if (activeSection.value === 'generation' && mediaDeps.value === null) {
      void refreshMediaDeps()
    }
  }, 400)
  }

  watch(activeSection, section => {
  if (section === 'generation' && mediaDeps.value === null) {
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

  function selectAgentModeModel(agentId: string, mode: PerformanceModeKey, value: string) {
    const parsed = splitProviderModelValue(value)
    if (parsed) {
      const next = { ...(s.settings.agentModeLlm ?? {}) }
      const agentMap = { ...(next[agentId] ?? {}) }
      agentMap[mode] = {
        providerId: parsed.providerId,
        model: parsed.model,
        ...thinkingPatchFromProviderModel(s.settings.providers, parsed.providerId, parsed.model)
      }
      next[agentId] = agentMap
      s.settings.agentModeLlm = next
      scheduleDebugModelSave()
      return
    }
    console.warn('[settings] selectAgentModeModel: invalid value', value)
  }

  function selectMediaModeModel(kind: MediaDebugKind, mode: PerformanceModeKey, value: string) {
    const parsed = splitProviderModelValue(value)
    if (parsed) {
      const next = { ...(s.settings.mediaModeLlm ?? {}) }
      const kindMap = { ...(next[kind] ?? {}) }
      kindMap[mode] = {
        providerId: parsed.providerId,
        model: parsed.model,
        ...thinkingPatchFromProviderModel(s.settings.providers, parsed.providerId, parsed.model)
      }
      next[kind] = kindMap
      s.settings.mediaModeLlm = next
      scheduleDebugModelSave()
      return
    }
    console.warn('[settings] selectMediaModeModel: invalid value', value)
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

  /**
   * 调试开关只控制调试入口可见性。
   * 其它运行时开关及智能体显示覆盖保持原值，避免打开/关闭设置影响正常默认行为。
   */
  async function toggleDebugMenus() {
  const next = !debugMenusEnabled.value
  debugMenusEnabled.value = next
  await s.save({ debugMenusEnabled: next })
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

  function saveTerminalEnvRows() {
    const next = terminalEnvOverridesFromRows()
    s.settings.terminalEnvOverrides = next
    void s.saveUser({ terminalEnvOverrides: next }).catch(error => {
      console.error('[settings] failed to save terminalEnvOverrides', error)
    })
  }

  function optionalParallelLimit(v: number | ''): number | null {
    if (v === '') return null
    const n = Number(v)
    if (!Number.isFinite(n) || n < 1) return null
    const limit = Math.floor(n)
    if (limit === autoParallelLimit) return null
    return limit
  }

  function clampParallelLimit(raw: unknown): number {
    const n = Number(raw)
    if (!Number.isFinite(n) || n < 1) return autoParallelLimit
    return Math.min(64, Math.max(1, Math.floor(n)))
  }

  /**
   * Number steppers commit here (not via the deep watch) so intermediate empty/NaN
   * values never hit Pinia, and we soft-patch only the one field before debounced IPC.
   */
  function commitMaxParallelToolCalls(raw: unknown) {
    const next = clampParallelLimit(raw)
    if (next === maxParallelToolCalls.value) return
    maxParallelToolCalls.value = next
    const stored = optionalParallelLimit(next)
    s.settings.maxParallelToolCalls = stored
    s.userSettings.maxParallelToolCalls = stored
    scheduleAssistantSave()
  }

  function commitMaxParallelSubAgents(raw: unknown) {
    const next = clampParallelLimit(raw)
    if (next === maxParallelSubAgents.value) return
    maxParallelSubAgents.value = next
    const stored = optionalParallelLimit(next)
    s.settings.maxParallelSubAgents = stored
    s.userSettings.maxParallelSubAgents = stored
    scheduleAssistantSave()
  }

  function commitMaxParallelMediaJobs(raw: unknown) {
    const next = clampParallelLimit(raw)
    if (next === maxParallelMediaJobs.value) return
    maxParallelMediaJobs.value = next
    const stored = optionalParallelLimit(next)
    s.settings.maxParallelMediaJobs = stored
    s.userSettings.maxParallelMediaJobs = stored
    scheduleAssistantSave()
  }

  function assistantPreferencesPayload() {
  return {
    computerAutoCompact: computerAutoCompact.value,
    collapseProcessByDefault: collapseProcessByDefault.value,
    userCodingRules: userCodingRules.value.trim(),
    toolApprovalMode: toolApprovalMode.value,
    computerHumanLike: computerHumanLike.value,
    computerAutoSwitchMonitor: computerAutoSwitchMonitor.value,
    computerInitialTier: computerInitialTier.value,
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
    contextCompressionEnabled: true,
    contextKeepRecentUserTurns: Number(contextKeepRecentUserTurns.value),
    maxToolRounds: Number(maxToolRounds.value),
    maxSubAgentToolRounds: migrateSubAgentToolRounds(maxSubAgentToolRounds.value),
    fileReadMaxBytes: Math.min(1024 * 1024, Math.max(4096, Math.round(Number(fileReadMaxKb.value) || 64) * 1024)),
    fileLineMaxBytes: Math.min(16 * 1024, Math.max(256, Math.floor(Number(fileLineMaxBytes.value) || 1024))),
    fileGrepMaxResults: Math.min(200, Math.max(1, Math.floor(Number(fileGrepMaxResults.value) || 50))),
    terminalOutputMaxBytes: Math.min(
      256 * 1024,
      Math.max(4096, Math.round(Number(terminalOutputMaxKb.value) || 16) * 1024)
    ),
    terminalTimeoutSeconds: Math.min(
      86_400,
      Math.max(1, Math.round(Number(terminalTimeoutSeconds.value) || 30))
    ),
    terminalMaxWallHours: Math.min(
      10_000,
      Math.max(1, Math.round(Number(terminalMaxWallHours.value) || 24))
    ),
    attachmentUploadMaxBytes: Math.min(
      512 * 1024 * 1024,
      Math.max(1024 * 1024, Math.round(Number(attachmentUploadMaxMb.value) || 100) * 1024 * 1024)
    ),
    agentPerformanceModes: { ...agentPerformanceModesLocal.value },
    mediaUnderstandingModes: { ...mediaUnderstandingModesLocal.value },
    rawContentViewEnabled: rawContentViewEnabled.value,
    computerAnnotatedScreenViewEnabled: computerAnnotatedScreenViewEnabled.value,
    taskBoardShowChildBoards: taskBoardShowChildBoards.value,
    agentUiOverrides: { ...(s.settings.agentUiOverrides ?? {}) },
  }
  }

  let autosaveReady = false
  let assistantSaveTimer: number | undefined
  let debugSaveTimer: number | undefined

  function applyAssistantPrefsToRuntime() {
    const payload = assistantPreferencesPayload()
    const prevModes = s.settings.agentPerformanceModes ?? {}
    // 弹窗关闭再打开从 settings / userSettings 回填。payload 里的项必须两边都写，
    // 否则后续只带部分字段的 saveUser 会用旧 userSettings 把刚改的值盖掉。
    s.settings.toolApprovalMode = payload.toolApprovalMode
    s.settings.computerInitialTier = payload.computerInitialTier
    s.settings.agentPerformanceModes = { ...payload.agentPerformanceModes }
    s.settings.mediaUnderstandingModes = { ...payload.mediaUnderstandingModes }
    s.settings.computerHumanLike = payload.computerHumanLike
    s.settings.computerAutoSwitchMonitor = payload.computerAutoSwitchMonitor
    s.settings.fileReadMaxBytes = payload.fileReadMaxBytes
    s.settings.fileLineMaxBytes = payload.fileLineMaxBytes
    s.settings.fileGrepMaxResults = payload.fileGrepMaxResults
    s.settings.terminalOutputMaxBytes = payload.terminalOutputMaxBytes
    s.settings.terminalTimeoutSeconds = payload.terminalTimeoutSeconds
    s.settings.terminalMaxWallHours = payload.terminalMaxWallHours
    s.settings.attachmentUploadMaxBytes = payload.attachmentUploadMaxBytes
    s.settings.maxToolRounds = payload.maxToolRounds
    s.settings.maxSubAgentToolRounds = payload.maxSubAgentToolRounds
    s.settings.contextCompressionEnabled = payload.contextCompressionEnabled
    s.settings.contextKeepRecentUserTurns = payload.contextKeepRecentUserTurns
    s.settings.parallelToolExecutionEnabled = payload.parallelToolExecutionEnabled
    s.settings.maxParallelToolCalls = payload.maxParallelToolCalls
    s.settings.maxParallelSubAgents = payload.maxParallelSubAgents
    s.settings.maxParallelMediaJobs = payload.maxParallelMediaJobs
    s.settings.maxConcurrentRuns = payload.maxConcurrentRuns
    s.settings.rawContentViewEnabled = payload.rawContentViewEnabled
    s.settings.computerAnnotatedScreenViewEnabled = payload.computerAnnotatedScreenViewEnabled
    s.settings.taskBoardShowChildBoards = payload.taskBoardShowChildBoards
    s.settings.agentUiOverrides = { ...payload.agentUiOverrides }
    s.userSettings.computerAutoCompact = payload.computerAutoCompact
    s.userSettings.collapseProcessByDefault = payload.collapseProcessByDefault
    s.userSettings.userCodingRules = payload.userCodingRules
    s.userSettings.toolApprovalMode = payload.toolApprovalMode
    s.userSettings.computerInitialTier = payload.computerInitialTier
    s.userSettings.agentPerformanceModes = { ...payload.agentPerformanceModes }
    s.userSettings.mediaUnderstandingModes = { ...payload.mediaUnderstandingModes }
    s.userSettings.computerHumanLike = payload.computerHumanLike
    s.userSettings.computerAutoSwitchMonitor = payload.computerAutoSwitchMonitor
    s.userSettings.fileReadMaxBytes = payload.fileReadMaxBytes
    s.userSettings.fileLineMaxBytes = payload.fileLineMaxBytes
    s.userSettings.fileGrepMaxResults = payload.fileGrepMaxResults
    s.userSettings.terminalOutputMaxBytes = payload.terminalOutputMaxBytes
    s.userSettings.terminalTimeoutSeconds = payload.terminalTimeoutSeconds
    s.userSettings.terminalMaxWallHours = payload.terminalMaxWallHours
    s.userSettings.attachmentUploadMaxBytes = payload.attachmentUploadMaxBytes
    s.userSettings.maxToolRounds = payload.maxToolRounds
    s.userSettings.maxSubAgentToolRounds = payload.maxSubAgentToolRounds
    s.userSettings.contextCompressionEnabled = payload.contextCompressionEnabled
    s.userSettings.contextKeepRecentUserTurns = payload.contextKeepRecentUserTurns
    s.userSettings.parallelToolExecutionEnabled = payload.parallelToolExecutionEnabled
    s.userSettings.maxParallelToolCalls = payload.maxParallelToolCalls
    s.userSettings.maxParallelSubAgents = payload.maxParallelSubAgents
    s.userSettings.maxParallelMediaJobs = payload.maxParallelMediaJobs
    s.userSettings.maxConcurrentRuns = payload.maxConcurrentRuns
    s.userSettings.rawContentViewEnabled = payload.rawContentViewEnabled
    s.userSettings.computerAnnotatedScreenViewEnabled = payload.computerAnnotatedScreenViewEnabled
    s.userSettings.taskBoardShowChildBoards = payload.taskBoardShowChildBoards
    s.userSettings.agentUiOverrides = { ...payload.agentUiOverrides }
    const conv = chat.current
    if (conv) {
      const lead = chat.effectiveConversationLeadAgentId(conv)
      const mode = payload.agentPerformanceModes[lead]
      if (
        (mode === 'fast' || mode === 'standard' || mode === 'expert') &&
        mode !== prevModes[lead]
      ) {
        chat.setConversationPerformanceMode(mode)
      }
    }
    return payload
  }

  let assistantSaveInFlight = false
  let assistantSaveQueued = false

  async function persistAssistantPreferences() {
    const payload = applyAssistantPrefsToRuntime()
    try {
      await s.saveAgentPreferences(payload)
    } catch (error) {
      console.error('[settings] failed to save assistant preferences', error)
    }
  }

  function flushAssistantPreferences() {
    return (async () => {
      if (assistantSaveInFlight) {
        assistantSaveQueued = true
        return
      }
      assistantSaveInFlight = true
      try {
        do {
          assistantSaveQueued = false
          await persistAssistantPreferences()
        } while (assistantSaveQueued)
      } finally {
        assistantSaveInFlight = false
      }
    })()
  }

  function scheduleAssistantSave() {
    if (!autosaveReady) return
    // Do not apply to Pinia on every keystroke / spinner tick. The form binds
    // local refs; mutating settings + userSettings here re-renders the shell.
    if (assistantSaveTimer) window.clearTimeout(assistantSaveTimer)
    assistantSaveTimer = window.setTimeout(() => {
      assistantSaveTimer = undefined
      void flushAssistantPreferences()
    }, 350)
  }

  function scheduleDebugSave() {
    if (!autosaveReady) return
    if (debugSaveTimer) window.clearTimeout(debugSaveTimer)
    debugSaveTimer = window.setTimeout(() => {
      debugSaveTimer = undefined
      const debugDump = debugDumpLlmPrompts.value
      void s.save({ debugDumpLlmPrompts: debugDump }).catch(error => {
        console.error('[settings] failed to save debug preferences', error)
      })
    }, 250)
  }

  watch(
    [
      toolApprovalMode,
      contextKeepRecentUserTurns,
      maxToolRounds,
      maxSubAgentToolRounds,
      fileReadMaxKb,
      fileLineMaxBytes,
      fileGrepMaxResults,
      terminalOutputMaxKb,
      terminalTimeoutSeconds,
      terminalMaxWallHours,
      attachmentUploadMaxMb,
      parallelToolExecutionEnabled,
      maxConcurrentRuns,
      computerHumanLike,
      computerAutoSwitchMonitor,
      computerAutoCompact,
      collapseProcessByDefault,
      userCodingRules,
      computerInitialTier,
      rawContentViewEnabled,
      computerAnnotatedScreenViewEnabled,
      taskBoardShowChildBoards,
      agentPerformanceModesLocal,
      mediaUnderstandingModesLocal
    ],
    scheduleAssistantSave,
    { deep: true }
  )
  watch(debugDumpLlmPrompts, scheduleDebugSave)

  onScopeDispose(() => {
    if (assistantSaveTimer) {
      window.clearTimeout(assistantSaveTimer)
      assistantSaveTimer = undefined
      void flushAssistantPreferences()
    } else if (assistantSaveQueued) {
      void flushAssistantPreferences()
    }
    if (debugSaveTimer) window.clearTimeout(debugSaveTimer)
    if (debugModelSaveTimer) window.clearTimeout(debugModelSaveTimer)
  })

  return {
    s,
    platformAuth,
    chat,
    activeSection,
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
    contextKeepRecentUserTurns,
    maxToolRounds,
    fileReadMaxKb,
    fileLineMaxBytes,
    fileGrepMaxResults,
    terminalOutputMaxKb,
    terminalTimeoutSeconds,
    terminalMaxWallHours,
    attachmentUploadMaxMb,
    parallelToolExecutionEnabled,
    autoParallelLimit,
    maxParallelToolCalls,
    maxParallelSubAgents,
    maxParallelMediaJobs,
    commitMaxParallelToolCalls,
    commitMaxParallelSubAgents,
    commitMaxParallelMediaJobs,
    maxConcurrentRuns,
    maxSubAgentToolRounds,
    maxSubAgentSpawnDepth,
    rawContentViewEnabled,
    debugDumpLlmPrompts,
    terminalEnvRows,
    addTerminalEnvRow,
    removeTerminalEnvRow,
    saveTerminalEnvRows,
    taskBoardShowChildBoards,
    agentTaskBoardHistoryTrim,
    computerHumanLike,
    computerAutoSwitchMonitor,
    computerAutoCompact,
    collapseProcessByDefault,
    userCodingRules,
    computerInitialTier,
    computerAnnotatedScreenViewEnabled,
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
    activeUiAgentId,
    activeUiAgentLabel,
    effectiveDisplayUi,
    platformAccountTitle,
    platformLogoutBusy,
    showDebugMenus,
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
  }
}
