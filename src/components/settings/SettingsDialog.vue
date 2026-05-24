<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  Bot,
  Bug,
  Check,
  ChevronRight,
  Copy,
  Cpu,
  Database,
  Gauge,
  Info,
  Monitor,
  Moon,
  Network,
  Plus,
  SlidersHorizontal,
  Sparkles,
  Sun,
  Trash2,
  UserCircle,
  Users,
  Wrench,
  X
} from 'lucide-vue-next'
import type {
  AgentDef,
  AgentUiConfig,
  ComputerInitialTier,
  ComputerTierKey,
  ComputerTierLlmConfig,
  ModelRuntimeOverrides,
  ProviderConfig,
  ThemePreference
} from '../../types/chat'
import { COMPUTER_INITIAL_TIER_OPTIONS } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../../types/chat'
import { applyTheme } from '../../lib/theme'
import { resolveAgentUi, composerAgentLabel } from '../../lib/agentUi'
import { TEAM_MODE_UI_ENABLED } from '../../lib/agentIcons'
import { listAgents } from '../../lib/api'
import {
  detectProviderTemplateId,
  isDeepSeekProvider,
  isQwenProvider,
  PROVIDER_TEMPLATE_OPTIONS,
  providerDraftForTemplate,
  providerTemplateMeta,
  stripProviderExtensionFields,
  type ProviderTemplateId
} from '../../lib/providerParams'
import {
  buildCustomModelEntryFromProvider,
  DEFAULT_MODEL_MAX_TOKENS,
  DEFAULT_MODEL_TEMPERATURE,
  hasEffectiveModelOverride,
  pruneInheritedModelConfigs,
  useRuntimeParams
} from '../../composables/useRuntimeParams'
import RuntimeParamsForm from './RuntimeParamsForm.vue'
import { isTauriRuntime } from '../../lib/runtime'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-skills'): void
  (e: 'platform-logout'): void
  (e: 'platform-login'): void
}>()
const s = useSettingsStore()
const platformAuth = usePlatformAuthStore()

const platformReadOnly = computed(() => !s.canEditPlatform)

const COMPUTER_TIER_UI: { key: ComputerTierKey; label: string }[] = [
  { key: 'primary', label: '初级' },
  { key: 'intermediate', label: '中级' },
  { key: 'advanced', label: '高级' }
]

const qwenModelOptions = computed(() => {
  const q = s.settings.providers.find(p => p.id === 'qwen')
  return q?.models?.length ? q.models : ['qwen3.5-plus', 'qwen3.6-plus']
})

function computerTierLlm(key: ComputerTierKey): ComputerTierLlmConfig {
  const m = s.platformSettings.computerTierLlm?.[key]
  return (
    m ?? {
      providerId: 'qwen',
      model: key === 'advanced' ? 'qwen3.6-plus' : 'qwen3.5-plus',
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

const saving = ref(false)
const activeSection = ref('assistant')
const copiedKey = ref(false)

const toolApprovalMode = ref<'auto' | 'manual'>('auto')
const agentMode = ref<'single' | 'supervisor'>('single')
const leadAgentId = ref('')
const contextCompressionEnabled = ref(true)
const contextBudgetChars = ref(120_000)
const contextKeepRecentUserTurns = ref(6)
const contextSummaryMaxTokens = ref(2048)
const maxToolRounds = ref(100)
const maxSubAgentToolRounds = ref(100)
const rawContentViewEnabled = ref(false)
const debugDumpLlmPrompts = ref(false)
const agentTaskBoardHistoryTrim = ref<Record<string, boolean>>({})
const computerHumanLike = ref(false)
const computerInitialTier = ref<ComputerInitialTier>('primary')
const computerAnnotatedScreenViewEnabled = ref(false)
const theme = ref<ThemePreference>('system')
const debugMenusEnabled = ref(false)
const agentUiLocal = ref<Partial<AgentUiConfig>>({})
const agents = ref<AgentDef[]>([])

const DISPLAY_UI_FIELDS: { key: keyof AgentUiConfig; label: string }[] = [
  { key: 'showAgentLabel', label: '消息旁显示智能体名称' },
  { key: 'showThoughts', label: '显示 thoughts 摘要' },
  { key: 'showHeadline', label: '显示 headline 标题条' },
  { key: 'showSubAgentTrace', label: '显示子任务进度时间线' },
  { key: 'showToolCalls', label: '显示工具调用卡片' },
  { key: 'showTaskBoardPanel', label: '显示任务板面板' },
  { key: 'showWorkspacePicker', label: 'Composer 显示工作区选择' },
  { key: 'showComputerMonitorPicker', label: 'Composer 显示显示器选择' }
]

const editingProvider = ref<ProviderConfig | null>(null)
const showAddProvider = ref(false)
const editingModelsText = ref('')
const originalApiKey = ref('')
const editingApiKey = ref('')

const modelConfigModalId = ref<string | null>(null)
const modelConfigModalError = ref('')
/** 服务商表单内「保存/添加」失败时的提示（勿静默 return）。 */
const providerSaveError = ref('')
/** 固定为 null：providerRuntimeApi 绑定服务商级默认，勿与 modelConfigModalId 混用。 */
const providerScopeModelId = ref<string | null>(null)
/** 与千问/深度求索相同的参数面板类型；新增服务商也须先选类型。 */
const providerTemplate = ref<ProviderTemplateId>('openai_compatible')

const globalGenFallback = {
  temperature: () => s.settings.temperature,
  maxTokens: () => s.settings.maxTokens
}

const providerRuntimeApi = useRuntimeParams(editingProvider, providerScopeModelId, globalGenFallback)
const modelRuntimeApi = useRuntimeParams(editingProvider, modelConfigModalId, globalGenFallback)

const providerTemplateHint = computed(
  () => providerTemplateMeta(providerTemplate.value).hint
)

watch(
  () =>
    editingProvider.value
      ? ([editingProvider.value.id, editingProvider.value.baseUrl] as const)
      : null,
  ids => {
    if (!ids || !editingProvider.value) return
    providerTemplate.value = detectProviderTemplateId(editingProvider.value)
  }
)

function maskKey(key: string): string {
  if (!key) return ''
  if (key.length <= 8) return '••••••••'
  return key.slice(0, 4) + '••••••••' + key.slice(-4)
}

const displayKey = computed(() => {
  if (!editingProvider.value) return ''
  if (showAddProvider.value) return editingApiKey.value
  if (editingApiKey.value) return editingApiKey.value
  return maskKey(originalApiKey.value)
})

const inputPlaceholder = computed(() => {
  if (showAddProvider.value) return '请输入 API 密钥'
  return '输入新密钥以替换原密钥'
})

function providerKeyDisplay(key: string): string {
  return key ? maskKey(key) : '未配置'
}

/** 浅拷贝 modelConfigs；写入时须先 clone 再赋回 editingProvider，勿在 template 渲染中创建条目（会死循环）。 */
function cloneModelConfigs(p?: ProviderConfig['modelConfigs']): NonNullable<ProviderConfig['modelConfigs']> {
  const src = p ?? {}
  const out: Record<string, ModelRuntimeOverrides> = {}
  for (const [k, v] of Object.entries(src)) {
    out[k] = { ...v }
  }
  return out
}


const editingParsedModelIds = computed(() =>
  editingModelsText.value
    .split(',')
    .map(m => m.trim())
    .filter(m => m.length > 0)
)

function modelConfigMode(modelId: string): 'same' | 'custom' {
  const p = editingProvider.value
  if (!p) return 'same'
  // 用户点「定制」会在 modelConfigs 写入条目；勿用 hasEffectiveModelOverride 判 UI 模式
  // （DeepSeek 未设 reasoningEffort 时与服务商默认相同，会被误判为「同上」导致「设置」无效）
  return p.modelConfigs?.[modelId] ? 'custom' : 'same'
}

function setModelConfigMode(modelId: string, mode: 'same' | 'custom') {
  if (!editingProvider.value) return
  if (mode === 'same') {
    // 必须整体替换 modelConfigs 对象，勿 delete 后省略赋回或就地改嵌套字段。
    const next = { ...(editingProvider.value.modelConfigs ?? {}) }
    delete next[modelId]
    editingProvider.value.modelConfigs = next
    if (modelConfigModalId.value === modelId) {
      closeModelConfigModal()
    }
    return
  }
  if (!editingProvider.value.modelConfigs?.[modelId]) {
    editingProvider.value.modelConfigs = {
      ...(editingProvider.value.modelConfigs ?? {}),
      [modelId]: buildCustomModelEntryFromProvider(editingProvider.value, globalGenFallback)
    }
  }
}

function openModelConfigModal(modelId: string) {
  if (!editingProvider.value || modelConfigMode(modelId) !== 'custom') return
  modelConfigModalError.value = ''
  if (!editingProvider.value.modelConfigs?.[modelId]) {
    setModelConfigMode(modelId, 'custom')
  }
  modelConfigModalId.value = modelId
}

function closeModelConfigModal() {
  modelConfigModalId.value = null
  modelConfigModalError.value = ''
}

/** 仅关闭单模型定制弹窗；勿 emit('close')，否则会退出整个设置对话框。 */
function confirmModelConfigModal() {
  modelConfigModalError.value = ''
  modelConfigModalId.value = null
}

function clearMaskedInput(e: Event) {
  if (showAddProvider.value || editingApiKey.value) return
  ;(e.target as HTMLInputElement).value = ''
}

function copyOriginalKey() {
  const key = originalApiKey.value
  if (!key) return
  navigator.clipboard.writeText(key).then(() => {
    copiedKey.value = true
    setTimeout(() => { copiedKey.value = false }, 2000)
  }).catch(e => console.error(e))
}

const alwaysSections = [
  { id: 'assistant', label: '智能体', desc: 'Computer 与工具权限', icon: Bot }
] as const

const debugSections = [
  { id: 'provider', label: '模型服务', desc: '管理 AI 服务', icon: Cpu },
  { id: 'generation', label: '界面配置', desc: '界面与调试', icon: Gauge },
  { id: 'agent', label: '智能模式', desc: '工作方式', icon: Gauge },
  { id: 'runtime', label: '运行时', desc: '存储与网络', icon: Database }
] as const

const showDebugMenus = computed(() => s.canEditPlatform && debugMenusEnabled.value)
const debugSectionIds = new Set<string>(debugSections.map(s => s.id))
const persistedSectionIds = new Set<string>(['assistant'])
const isPersistedSection = computed(() => persistedSectionIds.has(activeSection.value))
const showFooterSave = computed(() => {
  if (activeSection.value === 'account' || activeSection.value === 'runtime') return false
  return activeSection.value === 'assistant' || (s.canEditPlatform && debugSectionIds.has(activeSection.value))
})
const footerSaveLabel = computed(() =>
  isPersistedSection.value ? '保存' : '保存(本次会话)'
)
const debugModeTitle = computed(() =>
  debugMenusEnabled.value ? '调试模式：已开启（点击关闭）' : '调试模式：已关闭（点击开启）'
)

const sections = computed(() => {
  const merged = showDebugMenus.value
    ? [...alwaysSections, ...debugSections]
    : [...alwaysSections]
  if (!isTauriRuntime()) return merged
  return [
    { id: 'account', label: '平台账户', desc: '登录与凭据', icon: UserCircle },
    ...merged
  ]
})

const platformAccountTitle = computed(() => {
  if (!platformAuth.session.logged_in) return '未登录'
  return platformAuth.session.user_nickname?.trim() || '已登录'
})

const platformLogoutBusy = ref(false)

async function logoutPlatformAccount() {
  platformLogoutBusy.value = true
  try {
    await platformAuth.logout()
    emit('platform-logout')
  } catch (e) {
    console.error('[settings] platform logout failed', e)
  } finally {
    platformLogoutBusy.value = false
  }
}

const workers = computed(() => agents.value.filter(a => a.role === 'worker' && a.enabled))

const supervisorAgent = computed(
  () =>
    agents.value.find(a => a.id === 'supervisor' && a.enabled) ||
    agents.value.find(a => a.role === 'supervisor')
)

const activeUiAgentId = computed(() =>
  agentMode.value === 'supervisor' ? 'supervisor' : (leadAgentId.value?.trim() || DEFAULT_LEAD_AGENT_ID)
)

const displayUiFieldsForAgent = computed(() => {
  if (activeUiAgentId.value === 'computer') return DISPLAY_UI_FIELDS
  return DISPLAY_UI_FIELDS.filter(f => f.key !== 'showComputerMonitorPicker')
})

const activeUiAgentLabel = computed(() => {
  const id = activeUiAgentId.value
  if (id === 'supervisor') return composerAgentLabel(supervisorAgent.value, s.settings)
  const agent = workers.value.find(w => w.id === id) ?? workers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return composerAgentLabel(agent, s.settings)
})

const effectiveDisplayUi = computed(() => {
  const id = activeUiAgentId.value
  const agent =
    agentMode.value === 'supervisor'
      ? supervisorAgent.value
      : workers.value.find(w => w.id === id) ?? workers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
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
    showAgentLabel: effectiveDisplayUi.value.showAgentLabel,
    showThoughts: effectiveDisplayUi.value.showThoughts,
    showHeadline: effectiveDisplayUi.value.showHeadline,
    showSubAgentTrace: effectiveDisplayUi.value.showSubAgentTrace,
    showToolCalls: effectiveDisplayUi.value.showToolCalls,
    showTaskBoardPanel: effectiveDisplayUi.value.showTaskBoardPanel,
    showWorkspacePicker: effectiveDisplayUi.value.showWorkspacePicker,
    showComputerMonitorPicker: effectiveDisplayUi.value.showComputerMonitorPicker
  }
  return map[key as string] ?? true
}

function setDisplayUi(key: keyof AgentUiConfig, checked: boolean) {
  agentUiLocal.value = { ...agentUiLocal.value, [key]: checked }
}

async function applyThemeChoice(t: ThemePreference) {
  theme.value = t
  applyTheme(t)
  s.settings.theme = t
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

onMounted(() => {
  toolApprovalMode.value = s.settings.toolApprovalMode || 'auto'
  agentMode.value =
    !TEAM_MODE_UI_ENABLED && s.settings.agentMode === 'supervisor'
      ? 'single'
      : (s.settings.agentMode || 'single')
  leadAgentId.value = s.settings.leadAgentId || DEFAULT_LEAD_AGENT_ID
  contextCompressionEnabled.value = s.settings.contextCompressionEnabled !== false
  contextBudgetChars.value = s.settings.contextBudgetChars ?? 120_000
  contextKeepRecentUserTurns.value = s.settings.contextKeepRecentUserTurns ?? 6
  contextSummaryMaxTokens.value = s.settings.contextSummaryMaxTokens ?? 2048
  maxToolRounds.value = s.settings.maxToolRounds ?? 100
  maxSubAgentToolRounds.value = s.settings.maxSubAgentToolRounds ?? s.settings.maxToolRounds ?? 100
  rawContentViewEnabled.value = s.settings.rawContentViewEnabled === true
  debugDumpLlmPrompts.value = s.settings.debugDumpLlmPrompts === true
  agentTaskBoardHistoryTrim.value = { ...(s.settings.agentTaskBoardHistoryTrim ?? {}) }
  computerHumanLike.value = s.settings.computerHumanLike === true
  computerInitialTier.value = s.settings.computerInitialTier ?? 'primary'
  computerAnnotatedScreenViewEnabled.value = s.settings.computerAnnotatedScreenViewEnabled === true
  theme.value = (s.settings.theme as ThemePreference) || 'system'
  debugMenusEnabled.value = s.canEditPlatform
    && (s.settings.rawContentViewEnabled === true || s.settings.debugDumpLlmPrompts === true)
  agentUiLocal.value = { ...(s.settings.agentUiOverrides?.[activeUiAgentId.value] ?? {}) }
  loadAgents()
})

watch(activeUiAgentId, id => {
  agentUiLocal.value = { ...(s.settings.agentUiOverrides?.[id] ?? {}) }
})

watch(showDebugMenus, enabled => {
  if (!enabled) {
    if (debugSectionIds.has(activeSection.value as (typeof debugSections)[number]['id'])) {
      activeSection.value = 'assistant'
    }
  }
})

function onComputerInitialTierChecked(value: ComputerInitialTier, checked: boolean) {
  if (checked) computerInitialTier.value = value
}

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

function globalGenDefaults() {
  const t = s.settings.temperature
  const n = s.settings.maxTokens
  return {
    temperature: Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE,
    maxTokens: n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS
  }
}

function setProviderTemplate(template: ProviderTemplateId) {
  providerTemplate.value = template
  const ep = editingProvider.value
  if (!ep) return
  const g = globalGenDefaults()
  if (showAddProvider.value) {
    const draft = providerDraftForTemplate(template, g)
    editingProvider.value = stripProviderExtensionFields(
      {
        ...draft,
        apiKey: ep.apiKey || draft.apiKey,
        modelConfigs: cloneModelConfigs(ep.modelConfigs)
      },
      template
    )
    editingModelsText.value = (editingProvider.value.models ?? []).join(', ')
    return
  }
  const meta = providerTemplateMeta(template)
  editingProvider.value = stripProviderExtensionFields(
    {
      ...ep,
      id: ep.id.trim() || meta.defaultId,
      name: ep.name.trim() || meta.defaultName,
      baseUrl: ep.baseUrl.trim() || meta.defaultBaseUrl
    },
    template
  )
}

function startEditProvider(provider: ProviderConfig) {
  // 从 store 拷贝草稿，勿把列表里的 provider 对象直接赋给 editingProvider（保存时会互相覆盖）。
  const pruned = pruneInheritedModelConfigs(provider, provider.modelConfigs, globalGenFallback)
  const template = detectProviderTemplateId(provider)
  providerTemplate.value = template
  editingProvider.value = stripProviderExtensionFields(
    {
      ...provider,
      models: [...(provider.models ?? [])],
      modelConfigs: cloneModelConfigs(pruned)
    },
    template
  )
  originalApiKey.value = provider.apiKey
  editingApiKey.value = ''
  editingModelsText.value = (provider.models ?? []).join(', ')
  showAddProvider.value = false
}

function startAddProvider() {
  providerTemplate.value = 'openai_compatible'
  const draft = providerDraftForTemplate('openai_compatible', globalGenDefaults())
  editingProvider.value = draft
  originalApiKey.value = ''
  editingApiKey.value = ''
  editingModelsText.value = (draft.models ?? []).join(', ')
  showAddProvider.value = true
}

function cancelEditProvider() {
  editingProvider.value = null
  showAddProvider.value = false
  modelConfigModalId.value = null
  modelConfigModalError.value = ''
  providerSaveError.value = ''
}

/** 将编辑区整理为可写入 store 的快照；勿把 editingProvider 引用直接传给 updateProvider。 */
function buildProviderSnapshotFromEditor(): ProviderConfig | null {
  const draft = editingProvider.value
  if (!draft?.id || !draft.name || !draft.baseUrl) return null

  const models = editingModelsText.value
    .split(',')
    .map(m => m.trim())
    .filter(m => m.length > 0)

  const snapshot: ProviderConfig = {
    ...draft,
    id: draft.id.trim(),
    name: draft.name.trim(),
    baseUrl: draft.baseUrl.trim(),
    models,
    apiKey: editingApiKey.value
      ? editingApiKey.value
      : showAddProvider.value
        ? draft.apiKey
        : originalApiKey.value,
    modelConfigs: { ...(draft.modelConfigs ?? {}) }
  }

  if (!isQwenProvider(snapshot)) {
    delete snapshot.enableThinking
    delete snapshot.thinkingBudget
  } else if (snapshot.enableThinking !== true) {
    delete snapshot.thinkingBudget
  }
  if (!isDeepSeekProvider(snapshot)) {
    delete snapshot.reasoningEffort
  }

  const nextMc: Record<string, ModelRuntimeOverrides> = {}
  const configs = snapshot.modelConfigs ?? {}
  for (const id of models) {
    const o = configs[id]
    if (!o) continue
    const clean: ModelRuntimeOverrides = {}
    if (o.reasoningInMessages !== undefined) clean.reasoningInMessages = o.reasoningInMessages
    if (o.temperature !== undefined) clean.temperature = o.temperature
    if (o.maxTokens !== undefined) clean.maxTokens = o.maxTokens
    if (isDeepSeekProvider(snapshot) && o.reasoningEffort !== undefined) {
      clean.reasoningEffort = o.reasoningEffort
    }
    if (isQwenProvider(snapshot) && o.enableThinking !== undefined) {
      clean.enableThinking = o.enableThinking
    }
    if (isQwenProvider(snapshot) && o.enableThinking === true && o.thinkingBudget !== undefined) {
      clean.thinkingBudget = o.thinkingBudget
    }
    if (
      Object.keys(clean).length
      && hasEffectiveModelOverride(clean, snapshot, globalGenFallback)
    ) {
      nextMc[id] = clean
    }
  }
  snapshot.modelConfigs = nextMc
  return snapshot
}

/** 将编辑区快照写入内存中的 providers；返回 false 表示校验失败未写入。 */
function applyProviderSnapshotToStore(
  snapshot: ProviderConfig,
  wasAdd: boolean,
  /** false：保存后收起编辑区（用于即将关闭设置对话框时，勿再 startEditProvider 造成闪动） */
  reopenEdit = true
): boolean {
  const id = snapshot.id.trim()
  if (wasAdd && s.settings.providers.some(p => p.id === id)) {
    providerSaveError.value = '服务 ID 已存在，请换一个 ID'
    return false
  }
  if (!wasAdd && !s.settings.providers.some(p => p.id === id)) {
    providerSaveError.value = '找不到要更新的服务商，请取消后重新编辑'
    return false
  }

  if (wasAdd) {
    s.addProvider(snapshot)
  } else {
    s.updateProvider(id, snapshot)
  }

  modelConfigModalId.value = null
  modelConfigModalError.value = ''

  if (reopenEdit) {
    // 保存后继续编辑：用 store 副本重新打开，勿将 editingProvider 置 null（会像「配置界面空白」）。
    const saved = s.settings.providers.find(p => p.id === id)
    if (saved) {
      startEditProvider(saved)
    } else {
      editingProvider.value = null
      showAddProvider.value = false
    }
  } else {
    editingProvider.value = null
    showAddProvider.value = false
  }
  return true
}

/** 若正在编辑服务商，先把草稿（含模型列表 editingModelsText）合并进 store。 */
function flushEditingProviderToStore(reopenEdit = false): boolean {
  if (!editingProvider.value) return true
  const snapshot = buildProviderSnapshotFromEditor()
  if (!snapshot) {
    providerSaveError.value = '请填写服务 ID、名称和 API 地址'
    return false
  }
  return applyProviderSnapshotToStore(snapshot, showAddProvider.value, reopenEdit)
}

async function saveProvider() {
  providerSaveError.value = ''
  const snapshot = buildProviderSnapshotFromEditor()
  if (!snapshot) {
    providerSaveError.value = '请填写服务 ID、名称和 API 地址'
    return
  }

  const wasAdd = showAddProvider.value
  // 写入 store 后退出编辑区（回到服务商列表）；勿 emit('close')——底部「保存配置」才关闭整个设置对话框。
  if (!applyProviderSnapshotToStore(snapshot, wasAdd, false)) return

  try {
    await s.saveModelService({
      providers: s.settings.providers,
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model,
      temperature: s.settings.temperature,
      maxTokens: s.settings.maxTokens
    })
    providerSaveError.value = ''
  } catch (e) {
    console.error(e)
    providerSaveError.value = '应用配置失败，请重试'
  }
}


function removeProvider(id: string) {
  s.removeProvider(id)
  if (editingProvider.value?.id === id) {
    editingProvider.value = null
    showAddProvider.value = false
    modelConfigModalId.value = null
    modelConfigModalError.value = ''
  }
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

async function saveFromFooter() {
  saving.value = true
  providerSaveError.value = ''
  try {
    if (activeSection.value === 'provider') {
      if (editingProvider.value && !flushEditingProviderToStore()) {
        return
      }
      await s.saveModelService({
        providers: s.settings.providers,
        activeProviderId: s.settings.activeProviderId,
        model: s.settings.model,
        temperature: s.settings.temperature,
        maxTokens: s.settings.maxTokens,
        computerTierLlm: { ...s.platformSettings.computerTierLlm }
      })
    } else if (activeSection.value === 'assistant') {
      await s.saveAgentPreferences({
        toolApprovalMode: toolApprovalMode.value,
        computerHumanLike: computerHumanLike.value,
        computerInitialTier: computerInitialTier.value,
        contextCompressionEnabled: contextCompressionEnabled.value,
        contextBudgetChars: Number(contextBudgetChars.value),
        contextKeepRecentUserTurns: Number(contextKeepRecentUserTurns.value),
        contextSummaryMaxTokens: Number(contextSummaryMaxTokens.value),
        maxToolRounds: Number(maxToolRounds.value)
      })
    } else {
      if (editingProvider.value && !flushEditingProviderToStore()) {
        activeSection.value = 'provider'
        return
      }
      await s.saveSession({
        agentMode: agentMode.value,
        leadAgentId: agentMode.value === 'supervisor' ? '' : leadAgentId.value,
        maxSubAgentToolRounds: Number(maxSubAgentToolRounds.value),
        rawContentViewEnabled: rawContentViewEnabled.value,
        debugDumpLlmPrompts: debugDumpLlmPrompts.value,
        computerAnnotatedScreenViewEnabled: computerAnnotatedScreenViewEnabled.value,
        agentTaskBoardHistoryTrim: { ...agentTaskBoardHistoryTrim.value },
        agentUiOverrides: {
          ...(s.settings.agentUiOverrides ?? {}),
          [activeUiAgentId.value]: { ...agentUiLocal.value }
        }
      })
    }
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="modelConfigModalId ? closeModelConfigModal() : emit('close')">
    <div class="w-[960px] max-w-[94vw] h-[740px] max-h-[90vh] glass-strong rounded-2xl border border-border shadow-2xl flex flex-col overflow-hidden">
      <!-- Header -->
      <header class="px-6 h-14 flex items-center gap-2 border-b border-border shrink-0">
        <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
          <SlidersHorizontal class="w-4 h-4 text-accent" />
        </div>
        <div>
          <h2 class="text-base font-semibold text-foreground">设置</h2>
        </div>
        <div class="flex-1" />
        <button
          type="button"
          class="h-7 w-7 mr-1 rounded-md border border-border hover:bg-hover transition-colors inline-flex items-center justify-center cursor-pointer"
          title="技能管理"
          @click="emit('open-skills')"
        >
          <Sparkles class="w-4 h-4 text-accent" />
        </button>
        <div class="mr-1">
          <button
            type="button"
            class="h-7 w-7 rounded-md border border-border text-foreground hover:bg-hover transition-colors inline-flex items-center justify-center"
            :title="`主题：${themeLabel(theme)}（点击切换）`"
            @click="cycleTheme"
          >
            <component
              :is="currentThemeIcon"
              class="w-4 h-4"
              :class="theme === 'light' ? 'text-amber-400' : theme === 'dark' ? 'text-indigo-400' : 'text-emerald-400'"
            />
          </button>
        </div>
        <label
          v-if="s.canEditPlatform"
          class="mr-1"
        >
          <button
            type="button"
            class="h-7 w-7 rounded-md border border-border hover:bg-hover transition-colors inline-flex items-center justify-center"
            :title="debugModeTitle"
            @click="debugMenusEnabled = !debugMenusEnabled"
          >
            <Bug class="w-4 h-4" :class="debugMenusEnabled ? 'text-amber-400' : 'text-muted'" />
          </button>
        </label>
        <button class="p-1.5 rounded-lg hover:bg-hover cursor-pointer transition-colors" @click="emit('close')">
          <X class="w-4 h-4 text-muted" />
        </button>
      </header>

      <div class="flex flex-1 min-h-0">
        <!-- Sidebar -->
        <aside class="w-56 shrink-0 border-r border-border p-3 bg-[hsl(var(--card-elevated))]">
          <button
            v-for="item in sections"
            :key="item.id"
            class="w-full flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-all cursor-pointer group"
            :class="activeSection === item.id ? 'bg-accent/10 border border-accent/30' : 'border border-transparent hover:bg-hover'"
            @click="activeSection = item.id"
          >
            <div class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors"
                 :class="activeSection === item.id ? 'bg-accent/15' : 'bg-hover group-hover:bg-hover'">
              <component :is="item.icon" class="w-3.5 h-3.5" :class="activeSection === item.id ? 'text-accent' : 'text-muted'" />
            </div>
            <span class="min-w-0">
              <span class="block text-[13px] font-medium" :class="activeSection === item.id ? 'text-foreground' : 'text-foreground/80'">{{ item.label }}</span>
              <span class="block text-[11px] text-muted truncate">{{ item.desc }}</span>
            </span>
          </button>
        </aside>

        <!-- Main Content -->
        <main class="flex-1 overflow-y-auto">
          <p
            v-if="platformReadOnly && activeSection !== 'account' && activeSection !== 'runtime'"
            class="mx-6 mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-muted"
          >
            仅平台管理员可修改平台配置；重启后恢复默认。登录后 API 密钥由平台自动注入。
          </p>
          <!-- ==================== Assistant Section ==================== -->
          <section v-if="activeSection === 'assistant'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />智能体
              </h3>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <h4 class="text-sm font-medium text-foreground">电脑操控</h4>
              <div class="grid grid-cols-2 gap-3">
                <div class="px-1 py-1 inline-flex items-center gap-2">
                  <label class="inline-flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      class="rounded border-border bg-card text-accent focus:ring-accent/40"
                      :checked="computerHumanLike"
                      @change="computerHumanLike = ($event.target as HTMLInputElement).checked"
                    />
                    <span class="text-[12px] text-foreground">人性化鼠标移动</span>
                  </label>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                    title="启用后鼠标沿曲线移动并带微抖动；关闭时使用直线匀速移动（约 0.5–1.5 秒随机）"
                    aria-label="人性化鼠标移动说明"
                    @click.stop
                  >
                    <Info class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <div
                  class="px-1 py-1"
                  title="新会话开始时电脑操控智能体使用的视觉级别；会话中仍可能因验证失败自动升档"
                >
                  <div class="flex items-center gap-3">
                    <span class="text-[12px] text-foreground whitespace-nowrap">初始级别</span>
                    <label
                      v-for="opt in COMPUTER_INITIAL_TIER_OPTIONS"
                      :key="opt.value"
                      class="inline-flex items-center gap-1.5 cursor-pointer text-[11px] text-muted"
                    >
                      <input
                        type="checkbox"
                        class="rounded border-border bg-card text-accent focus:ring-accent/40"
                        :checked="computerInitialTier === opt.value"
                        @change="onComputerInitialTierChecked(opt.value, ($event.target as HTMLInputElement).checked)"
                      />
                      <span class="text-foreground">{{ opt.label }}</span>
                    </label>
                  </div>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Wrench class="w-4 h-4 text-accent" />工具使用权限
              </h4>
              <div class="grid grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'auto' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-foreground">自动执行</span>
                  <span class="mt-1 block text-[11px] text-muted">AI 使用工具时自动执行，无需确认</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'manual' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-foreground">敏感操作确认</span>
                  <span class="mt-1 block text-[11px] text-muted">涉及文件、命令等操作时需要你确认</span>
                </label>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div class="flex items-center justify-between">
                <h4 class="text-sm font-medium text-foreground">上下文自动压缩</h4>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="contextCompressionEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track"></div>
                </label>
              </div>
              <p class="text-[11px] text-muted">当历史消息超过预算时，自动生成摘要并保留最近若干轮对话原文。</p>

              <div v-if="contextCompressionEnabled" class="grid grid-cols-2 gap-3 pt-2 border-t border-border">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">触发预算（字符）</label>
                  <input v-model.number="contextBudgetChars" type="number" min="8000" max="2000000" step="1000" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">保留最近用户轮数</label>
                  <input v-model.number="contextKeepRecentUserTurns" type="number" min="1" max="50" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">摘要最大 tokens</label>
                  <input v-model.number="contextSummaryMaxTokens" type="number" min="128" max="8192" step="64" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">单轮最大工具调用轮次</label>
                  <input v-model.number="maxToolRounds" type="number" min="1" max="10000" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
              </div>
            </div>
          </section>

          <!-- ==================== Provider Section ==================== -->
          <section v-else-if="activeSection === 'provider'" class="p-6 space-y-5">
            <div class="flex items-center justify-between">
              <div>
                <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                  <Cpu class="w-4 h-4 text-accent" />模型服务
                </h3>
                <p class="mt-0.5 text-xs text-muted">管理 AI 模型服务的连接配置</p>
              </div>
              <button
                class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent/10 hover:bg-accent/20 text-[12px] text-accent cursor-pointer transition-colors"
                @click="startAddProvider"
              >
                <Plus class="w-3.5 h-3.5" />
                添加服务
              </button>
            </div>

            <!-- Provider List -->
            <div class="space-y-2">
              <div
                v-for="p in s.settings.providers"
                :key="p.id"
                class="group relative rounded-xl border p-4 transition-all"
                :class="s.settings.activeProviderId === p.id ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
              >
                <div class="flex items-start gap-3">
                  <!-- Status indicator -->
                  <div class="mt-0.5 w-2 h-2 rounded-full shrink-0" :class="p.apiKey ? 'bg-success' : 'bg-warning'" />
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ p.name }}</span>
                      <span v-if="s.settings.activeProviderId === p.id" class="px-1.5 py-0.5 rounded bg-accent/15 text-[10px] font-medium text-accent">使用中</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted truncate font-mono">{{ p.baseUrl }}</p>
                    <div class="mt-1 flex items-center gap-3 text-[11px] text-muted">
                      <span>密钥：{{ providerKeyDisplay(p.apiKey) }}</span>
                      <span class="text-border">|</span>
                      <!-- models 须 optional chain：normalize 前旧数据可能缺该字段，直接 .length 会导致整页白屏 -->
                      <span>模型：{{ (p.models?.length ?? 0) > 0 ? `${p.models!.length} 个` : '未配置' }}</span>
                    </div>
                  </div>
                  <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button class="p-1.5 rounded-lg hover:bg-hover cursor-pointer transition-colors" @click="startEditProvider(p)">
                      <Wrench class="w-3.5 h-3.5 text-muted" />
                    </button>
                    <button class="p-1.5 rounded-lg hover:bg-hover cursor-pointer transition-colors" @click="removeProvider(p.id)">
                      <Trash2 class="w-3.5 h-3.5 text-danger" />
                    </button>
                    <button v-if="s.settings.activeProviderId !== p.id" class="ml-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors" @click="s.setActiveProvider(p.id)">
                      使用
                    </button>
                  </div>
                </div>
              </div>

              <!-- Empty state -->
              <div v-if="s.settings.providers.length === 0" class="rounded-xl border border-dashed border-border p-8 text-center">
                <Cpu class="w-8 h-8 text-muted/80 mx-auto mb-2" />
                <p class="text-sm text-muted">暂无模型服务</p>
                <p class="text-xs text-muted/80 mt-1">点击上方"添加服务"开始配置</p>
              </div>
            </div>

            <!-- Edit/Add Form -->
            <div v-if="editingProvider" class="rounded-xl border border-accent/30 bg-accent/5 p-5 space-y-4">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <ChevronRight class="w-4 h-4 text-accent" />
                {{ showAddProvider ? '添加模型服务' : '编辑模型服务' }}
              </h4>

              <div class="space-y-2">
                <label class="block text-[12px] text-muted">服务类型</label>
                <div class="inline-flex flex-wrap gap-1 rounded-lg bg-card border border-border p-0.5">
                  <button
                    v-for="opt in PROVIDER_TEMPLATE_OPTIONS"
                    :key="opt.id"
                    type="button"
                    class="h-8 px-3 rounded-md text-[12px] cursor-pointer transition-colors"
                    :class="providerTemplate === opt.id ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
                    @click="setProviderTemplate(opt.id)"
                  >
                    {{ opt.label }}
                  </button>
                </div>
                <p class="text-[11px] text-muted">
                  与内置千问/深度求索相同：先设服务商默认参数，再在下方各模型选「同上」或「定制」。
                  <span class="text-muted">（{{ providerTemplateHint }}）</span>
                </p>
              </div>

              <div class="grid grid-cols-2 gap-3">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">服务 ID</label>
                  <input v-model="editingProvider.id" :disabled="!showAddProvider" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 disabled:opacity-50 transition-colors" placeholder="例如：qwen, openai" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">服务名称</label>
                  <input v-model="editingProvider.name" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" placeholder="例如：千问" />
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-muted mb-1.5">API 地址</label>
                  <input v-model="editingProvider.baseUrl" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" placeholder="https://api.example.com/v1" />
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-muted mb-1.5">API 密钥</label>
                  <div class="flex items-center gap-2 h-9 px-3 rounded-lg bg-card border border-border transition-colors focus-within:border-primary/50">
                    <input :value="displayKey" @focus="clearMaskedInput" @input="e => { editingApiKey = (e.target as HTMLInputElement).value }" :type="editingApiKey || showAddProvider ? 'password' : 'text'" class="flex-1 bg-transparent border-0 outline-none text-sm text-foreground placeholder:text-muted font-mono" :placeholder="inputPlaceholder" />
                    <button v-if="!showAddProvider && originalApiKey" class="p-1 rounded hover:bg-hover cursor-pointer transition" :class="copiedKey ? 'text-success' : 'text-muted hover:text-foreground'" @click="copyOriginalKey" :title="copiedKey ? '已复制' : '复制原始密钥'">
                      <Check v-if="copiedKey" class="w-4 h-4" />
                      <Copy v-else class="w-4 h-4" />
                    </button>
                  </div>
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-muted mb-1.5">模型列表</label>
                  <input v-model="editingModelsText" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" placeholder="model-1, model-2, model-3"  />
                </div>
                <div class="col-span-2 rounded-lg border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3">
                  <h5 class="text-[12px] font-medium text-foreground">模型参数</h5>
                  <p class="text-[11px] text-muted">服务商级默认；各模型可选「同上」或「定制」。</p>
                  <RuntimeParamsForm :api="providerRuntimeApi" />
                  <div v-if="editingParsedModelIds.length" class="pt-2 border-t border-border space-y-1.5">
                    <div class="text-[11px] text-muted">各模型</div>
                    <ul class="rounded-lg border border-border bg-hover divide-y divide-border overflow-hidden">
                      <li
                        v-for="mid in editingParsedModelIds"
                        :key="mid"
                        class="flex items-center gap-2 px-3 py-2 min-h-10"
                      >
                        <span class="flex-1 min-w-0 font-mono text-[12px] text-foreground truncate" :title="mid">{{ mid }}</span>
                        <div class="inline-flex rounded-lg bg-card border border-border p-0.5 shrink-0">
                          <button
                            type="button"
                            class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
                            :class="modelConfigMode(mid) === 'same' ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
                            @click="setModelConfigMode(mid, 'same')"
                          >同上</button>
                          <button
                            type="button"
                            class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
                            :class="modelConfigMode(mid) === 'custom' ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
                            @click="setModelConfigMode(mid, 'custom')"
                          >定制</button>
                        </div>
                        <button
                          v-if="modelConfigMode(mid) === 'custom'"
                          type="button"
                          class="shrink-0 h-7 px-2.5 rounded-md bg-hover hover:bg-hover text-[11px] text-foreground cursor-pointer transition-colors"
                          @click="openModelConfigModal(mid)"
                        >
                          设置
                        </button>
                      </li>
                    </ul>
                  </div>
                </div>
              </div>

              <div class="space-y-2 pt-1">
                <p v-if="providerSaveError" class="text-[12px] text-red-400">{{ providerSaveError }}</p>
                <div class="flex items-center justify-end gap-2">
                  <button class="h-8 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="cancelEditProvider">取消</button>
                  <button class="h-8 px-4 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity" :disabled="!editingProvider.id?.trim() || !editingProvider.name?.trim() || !editingProvider.baseUrl?.trim()" @click="saveProvider">
                    {{ showAddProvider ? '添加' : '保存' }}
                  </button>
                </div>
              </div>
            </div>
          </section>

          <!-- ==================== Generation Section ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Gauge class="w-4 h-4 text-accent" />界面配置
              </h3>
              <p class="mt-0.5 text-xs text-muted">界面显示与调试选项；模型创造性、最大输出等在模型服务中配置</p>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 text-[12px] text-muted leading-relaxed">
              请在 <span class="text-foreground">模型服务</span> 中编辑服务商，配置
              <span class="text-foreground">创造性</span>、
              <span class="text-foreground">最大输出</span> 等默认项；在「各模型」选择
              <span class="text-foreground">定制</span> 后点
              <span class="text-foreground">设置</span> 可单独覆盖。
              当前会话模型：
              <span class="font-mono text-accent">{{ s.settings.model }}</span>
              （创造性 {{ s.effectiveTemperature }}，最大输出 {{ s.effectiveMaxTokens }} tokens）。
            </div>

            <div
              v-if="showDebugMenus"
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4"
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-medium text-foreground">原始内容查看</h4>
                  <p class="mt-1 text-[11px] text-muted">在助手消息上显示「原始输出」入口（代码图标），展开后为一段可复制文本：含推理（若有）与正文通道原始输出，不在主气泡内展示。</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="rawContentViewEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track" />
                </label>
              </div>
            </div>

            <div
              v-if="showDebugMenus"
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4"
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-medium text-foreground">保存每轮对话请求</h4>
                  <p class="mt-1 text-[11px] text-muted">开启后，每次向 AI 发送的完整上下文会分别保存为本地文件（应用数据目录下的日志文件夹），便于排查问题；内嵌的大块图片内容会缩短显示。</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="debugDumpLlmPrompts" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track" />
                </label>
              </div>
            </div>

            <div
              v-if="showDebugMenus"
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4"
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-medium text-foreground">标记截图查看</h4>
                  <p class="mt-1 text-[11px] text-muted">开启后，Computer Use 助手消息上显示相机按钮，可查看带标注的桌面截图。</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="computerAnnotatedScreenViewEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track" />
                </label>
              </div>
            </div>
          </section>

          <!-- ==================== Agent Section ==================== -->
          <section v-else-if="activeSection === 'agent'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />智能模式
              </h3>
              <p class="mt-0.5 text-xs text-muted">选择 AI 的工作方式和工具使用权限</p>
            </div>

            <!-- Agent Cards -->
            <div class="space-y-2">
              <h4 class="text-[12px] font-medium text-muted uppercase tracking-wider">执行智能体</h4>

              <!-- Worker Agents -->
              <div
                v-for="w in workers"
                :key="w.id"
                class="rounded-xl border p-3 cursor-pointer transition-all"
                :class="isLeadWorkerSelected(w.id) ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
                @click="agentMode = 'single'; leadAgentId = w.id"
              >
                <div class="flex items-start gap-3">
                  <!-- Icon -->
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="isLeadWorkerSelected(w.id) ? 'bg-accent/10' : 'bg-[hsl(var(--card-elevated))]'">
                    <Bot class="w-4 h-4" :class="isLeadWorkerSelected(w.id) ? 'text-accent' : 'text-muted'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ composerAgentLabel(w, s.settings) }}</span>
                      <span class="px-1.5 py-0.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[10px] text-muted font-mono">{{ w.id }}</span>
                      <span v-if="isLeadWorkerSelected(w.id)" class="px-1.5 py-0.5 rounded bg-accent/15 text-[10px] font-medium text-accent">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted">{{ w.description || '通用智能体' }}</p>

                    <!-- Per-agent default model (lead or delegated sub-agent runs) -->
                    <div class="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-2" @click.stop>
                      <div class="flex items-center gap-2 min-w-0">
                        <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                        <span class="text-[11px] text-muted shrink-0">默认模型</span>
                        <select
                          :value="getAgentModelWithProvider(w.id)"
                          @change.stop="selectAgentModelWithProvider(w.id, ($event.target as HTMLSelectElement).value)"
                          @click.stop
                          class="w-48 h-7 px-2 rounded bg-card border border-border text-[11px] text-foreground cursor-pointer outline-none focus:border-accent/50 transition-colors"
                        >
                          <option value="">使用全局默认</option>
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                      </div>

                      <label class="inline-flex items-center gap-1.5 cursor-pointer shrink-0">
                        <input
                          type="checkbox"
                          class="rounded border-border bg-card text-accent focus:ring-accent/40"
                          :checked="taskBoardTrimChecked(w.id)"
                          @change="setTaskBoardTrimLocal(w.id, ($event.target as HTMLInputElement).checked)"
                        />
                        <span class="text-[11px] text-muted">任务板后精简历史</span>
                      </label>

                    </div>

                    <div
                      v-if="w.id === 'computer'"
                      class="col-span-full mt-3 rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
                      :class="platformReadOnly ? 'opacity-60 pointer-events-none' : ''"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <h4 class="text-xs font-medium text-foreground">电脑操控分级模型</h4>
                        <span class="text-[10px] text-muted">按级别覆盖模型与思考参数</span>
                      </div>
                      <div
                        v-for="tier in COMPUTER_TIER_UI"
                        :key="tier.key"
                        class="grid grid-cols-[4.5rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                      >
                        <span class="text-[11px] text-muted font-medium">{{ tier.label }}</span>
                        <select
                          :value="computerTierLlm(tier.key).model"
                          class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          @change="patchComputerTierLlm(tier.key, { model: ($event.target as HTMLSelectElement).value })"
                        >
                          <option v-for="m in qwenModelOptions" :key="m" :value="m">{{ m }}</option>
                        </select>
                        <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                          <input
                            type="checkbox"
                            class="rounded border-border bg-[hsl(var(--card-elevated))]"
                            :checked="computerTierLlm(tier.key).enableThinking !== false"
                            @change="patchComputerTierLlm(tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })"
                          />
                          思考
                        </label>
                        <input
                          type="number"
                          min="256"
                          step="256"
                          class="h-8 w-full px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          :value="computerTierLlm(tier.key).thinkingBudget ?? 2048"
                          :disabled="computerTierLlm(tier.key).enableThinking === false"
                          @change="patchComputerTierLlm(tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                        />
                      </div>
                    </div>

                  </div>
                </div>
              </div>

              <!-- 团队模式 -->
              <div
                v-if="supervisorAgent"
                class="rounded-xl border p-3 cursor-pointer transition-all"
                :class="agentMode === 'supervisor' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
                @click="agentMode = 'supervisor'"
              >
                <div class="flex items-start gap-3">
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="agentMode === 'supervisor' ? 'bg-accent/10' : 'bg-[hsl(var(--card-elevated))]'">
                    <Users class="w-4 h-4" :class="agentMode === 'supervisor' ? 'text-accent' : 'text-muted'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ composerAgentLabel(supervisorAgent, s.settings) }}</span>
                      <span v-if="agentMode === 'supervisor'" class="px-1.5 py-0.5 rounded bg-accent/15 text-[10px] font-medium text-accent">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted">多子智能体编排与结果整合</p>

                    <!-- Default Model Selector -->
                    <div class="mt-2.5 flex items-center gap-2" @click.stop>
                      <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                      <span class="text-[11px] text-muted shrink-0">默认模型</span>
                      <select
                        :value="getAgentModelWithProvider('supervisor')"
                        @change.stop="selectAgentModelWithProvider('supervisor', ($event.target as HTMLSelectElement).value)"
                        @click.stop
                        class="w-48 h-7 px-2 rounded bg-card border border-border text-[11px] text-foreground cursor-pointer outline-none focus:border-accent/50 transition-colors"
                      >
                        <option value="">使用全局默认</option>
                        <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground">聊天界面显示</h4>
              <p class="text-[11px] text-muted">
                覆盖当前选中智能体（{{ activeUiAgentLabel }}）的默认展示；未勾选项使用 AGENT.md 内置默认。
              </p>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
                <label
                  v-for="f in displayUiFieldsForAgent"
                  :key="f.key"
                  class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground"
                >
                  <input
                    type="checkbox"
                    class="rounded border-border text-accent focus:ring-accent/40"
                    :checked="displayUiChecked(f.key)"
                    @change="setDisplayUi(f.key, ($event.target as HTMLInputElement).checked)"
                  />
                  {{ f.label }}
                </label>
              </div>
            </div>

            <div
              v-if="agentMode === 'single'"
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3"
            >
              <h4 class="text-sm font-medium text-foreground">子任务委托</h4>
              <p class="text-[11px] text-muted">
                可委派的 worker 由主 Agent 的 AGENT.md 中 <code class="text-muted">allowAgents</code> 配置。
              </p>
              <div>
                <label class="block text-[12px] text-muted mb-1.5">子 Agent 内工具轮次上限</label>
                <input
                  v-model.number="maxSubAgentToolRounds"
                  type="number"
                  min="1"
                  max="10000"
                  step="1"
                  class="w-full max-w-xs h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
            </div>

          </section>

          <!-- ==================== Platform account (desktop) ==================== -->
          <section v-else-if="activeSection === 'account'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <UserCircle class="w-4 h-4 text-accent" />平台账户
              </h3>
              <p class="mt-0.5 text-xs text-muted">Pointer 平台登录状态</p>
            </div>

            <div class="rounded-xl border border-border panel p-5 space-y-4">
              <div class="flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <p class="text-sm font-medium text-foreground">{{ platformAccountTitle }}</p>
                  <p v-if="!platformAuth.session.logged_in" class="mt-1 text-xs text-muted">
                    登录后可使用平台相关能力
                  </p>
                </div>
                <span
                  v-if="platformAuth.session.logged_in"
                  class="shrink-0 rounded-md border border-success/30 bg-success/10 px-2 py-0.5 text-[11px] text-success"
                >
                  已登录
                </span>
              </div>

              <div class="flex flex-wrap gap-2">
                <button
                  v-if="platformAuth.session.logged_in"
                  type="button"
                  class="h-8 px-4 rounded-lg border border-border text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
                  :disabled="platformLogoutBusy"
                  @click="logoutPlatformAccount"
                >
                  {{ platformLogoutBusy ? '退出中…' : '退出登录' }}
                </button>
                <button
                  v-else
                  type="button"
                  class="h-8 px-4 rounded-lg bg-accent text-sm font-medium text-white hover:opacity-95 cursor-pointer transition-opacity"
                  @click="emit('platform-login')"
                >
                  浏览器登录
                </button>
              </div>
            </div>
          </section>

          <!-- ==================== Runtime Section ==================== -->
          <section v-else-if="activeSection === 'runtime'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Database class="w-4 h-4 text-accent" />运行时与存储
              </h3>
              <p class="mt-0.5 text-xs text-muted">查看当前存储与网络运行方式</p>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div class="rounded-xl border border-border panel p-5">
                <div class="w-9 h-9 rounded-lg bg-accent/10 flex items-center justify-center mb-3">
                  <Database class="w-4 h-4 text-accent" />
                </div>
                <div class="text-sm font-medium text-foreground">本地数据</div>
                <p class="mt-1 text-xs text-muted">平台账户凭据保存在 auth.dat；智能体配置保存在 local_platform_settings.json；其余配置仅在本次会话有效。</p>
              </div>
              <div class="rounded-xl border border-border panel p-5">
                <div class="w-9 h-9 rounded-lg bg-accent/10 flex items-center justify-center mb-3">
                  <Network class="w-4 h-4 text-accent" />
                </div>
                <div class="text-sm font-medium text-foreground">网络</div>
                <p class="mt-1 text-xs text-muted">当前直接访问 AI 服务 API。</p>
              </div>
            </div>
          </section>
        </main>
      </div>

      <!-- Footer -->
      <footer class="px-6 h-14 flex items-center justify-end gap-3 border-t border-border shrink-0">
        <button class="h-9 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="emit('close')">取消</button>
        <button
          v-if="showFooterSave"
          class="h-9 px-5 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity"
          :disabled="saving"
          @click="saveFromFooter"
        >
          {{ saving ? '保存中…' : footerSaveLabel }}
        </button>
      </footer>
    </div>

    <!-- Per-model overrides: Teleport avoids flex/stacking quirks; pointerdown opens before blur can drop click. -->
    <Teleport to="body">
      <div
        v-if="modelConfigModalId && editingProvider"
        class="pointer-events-auto fixed inset-0 z-[10001] flex items-center justify-center bg-black/55 p-4"
        role="presentation"
        @click.self="closeModelConfigModal"
      >
        <div class="w-full max-w-md rounded-xl border border-border bg-card shadow-2xl p-4 space-y-3" @click.stop>
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <h5 class="text-sm font-medium text-foreground">模型参数</h5>
            <p class="mt-0.5 text-[11px] text-muted font-mono truncate" :title="modelConfigModalId">{{ modelConfigModalId }}</p>
          </div>
          <button type="button" class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0" aria-label="关闭" @click="closeModelConfigModal">
            <X class="w-4 h-4" />
          </button>
        </div>
        <RuntimeParamsForm v-if="modelConfigModalId" :api="modelRuntimeApi" />
        <div class="flex items-center justify-end gap-2 pt-1">
          <button type="button" class="h-8 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="closeModelConfigModal">取消</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity" @click="confirmModelConfigModal">完成</button>
        </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
