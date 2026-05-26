<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  Bot,
  Bug,
  Cpu,
  Database,
  Gauge,
  Info,
  Monitor,
  Moon,
  Network,
  SlidersHorizontal,
  Sparkles,
  Sun,
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
  ThemePreference
} from '../../types/chat'
import { COMPUTER_INITIAL_TIER_OPTIONS } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../../types/chat'
import { applyTheme } from '../../lib/theme'
import { resolveAgentUi, composerAgentLabel } from '../../lib/agentUi'
import { TEAM_MODE_UI_ENABLED } from '../../lib/agentIcons'
import { listAgents } from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'
import ProviderSettingsPanel from './ProviderSettingsPanel.vue'

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-skills'): void
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
  { key: 'showThoughts', label: '显示 thoughts 摘要（调试，完成后保留）' },
  { key: 'showHeadline', label: '显示 headline 标题条' },
  { key: 'showSubAgentTrace', label: '显示子 Agent 边框面板' },
  { key: 'showToolCalls', label: '显示工具调用卡片' },
  { key: 'showToolCallResults', label: '显示工具调用结果（调试）' },
  { key: 'showTaskBoardPanel', label: '显示任务板面板' },
  { key: 'showWorkspacePicker', label: 'Composer 显示工作区选择' },
  { key: 'showComputerMonitorPicker', label: 'Composer 显示显示器选择' }
]

const alwaysSections = [
  { id: 'assistant', label: '智能体', desc: 'Computer 与工具权限', icon: Bot }
] as const

const debugSections = [
  { id: 'provider', label: '模型服务', desc: '管理 AI 服务', icon: Cpu },
  { id: 'generation', label: '界面配置', desc: '界面与调试', icon: Gauge },
  { id: 'agent', label: '智能模式', desc: '工作方式', icon: Gauge },
  { id: 'runtime', label: '运行时', desc: '存储与网络', icon: Database }
] as const

const providerPanelRef = ref<InstanceType<typeof ProviderSettingsPanel> | null>(null)

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
  } catch (e) {
    console.error('[settings] platform logout failed', e)
  } finally {
    platformLogoutBusy.value = false
  }
}

async function loginPlatformAccount() {
  try {
    await platformAuth.login()
  } catch (e) {
    console.error('[settings] platform login failed', e)
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
    showToolCallResults: effectiveDisplayUi.value.showToolCallResults,
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
  debugMenusEnabled.value = s.canEditPlatform && s.settings.debugMenusEnabled === true
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

async function toggleDebugMenus() {
  const next = !debugMenusEnabled.value
  debugMenusEnabled.value = next
  rawContentViewEnabled.value = next
  computerAnnotatedScreenViewEnabled.value = next
  await s.save({
    debugMenusEnabled: next,
    rawContentViewEnabled: next,
    computerAnnotatedScreenViewEnabled: next
  })
}

function onDialogBackdropClick() {
  if (providerPanelRef.value?.isModelConfigOpen()) {
    providerPanelRef.value.closeModelConfigModal()
    return
  }
  emit('close')
}

async function saveFromFooter() {
  saving.value = true
  try {
    if (activeSection.value === 'provider') {
      if (providerPanelRef.value?.hasUnsavedEdits() && !providerPanelRef.value.flushEditingProviderToStore()) {
        return
      }
      await s.saveModelService({
        providers: s.settings.providers,
        activeProviderId: s.settings.activeProviderId,
        model: s.settings.model,
        temperature: s.settings.temperature,
        maxTokens: s.settings.maxTokens
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
      if (providerPanelRef.value?.hasUnsavedEdits() && !providerPanelRef.value.flushEditingProviderToStore()) {
        activeSection.value = 'provider'
        return
      }
      await s.save({
        agentMode: agentMode.value,
        leadAgentId: agentMode.value === 'supervisor' ? '' : leadAgentId.value,
        maxSubAgentToolRounds: Number(maxSubAgentToolRounds.value),
        rawContentViewEnabled: rawContentViewEnabled.value,
        debugDumpLlmPrompts: debugDumpLlmPrompts.value,
        debugMenusEnabled: debugMenusEnabled.value,
        computerAnnotatedScreenViewEnabled: computerAnnotatedScreenViewEnabled.value,
        agentTaskBoardHistoryTrim: { ...agentTaskBoardHistoryTrim.value },
        agentUiOverrides: {
          ...(s.settings.agentUiOverrides ?? {}),
          [activeUiAgentId.value]: { ...agentUiLocal.value }
        },
        computerTierLlm: { ...s.platformSettings.computerTierLlm }
      })
    }
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="onDialogBackdropClick">
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
            @click="toggleDebugMenus"
          >
            <Bug class="w-4 h-4" :class="debugMenusEnabled ? 'text-amber-400' : 'text-muted'" />
          </button>
        </label>
        <button
          type="button"
          class="h-7 w-7 rounded-md border border-border text-foreground hover:bg-hover transition-colors inline-flex items-center justify-center cursor-pointer"
          title="关闭"
          aria-label="关闭"
          @click="emit('close')"
        >
          <X class="w-4 h-4" />
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

          <!-- ==================== Generation Section ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Gauge class="w-4 h-4 text-accent" />界面配置
              </h3>
              <p class="mt-0.5 text-xs text-muted">
                {{ showDebugMenus ? '界面显示与调试选项；模型创造性、最大输出等在模型服务中配置' : '界面显示与调试选项' }}
              </p>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 text-[12px] text-muted leading-relaxed">
              <template v-if="showDebugMenus">
                请在 <span class="text-foreground">模型服务</span> 中编辑服务商，配置
                <span class="text-foreground">创造性</span>、
                <span class="text-foreground">最大输出</span> 等默认项；在「各模型」选择
                <span class="text-foreground">定制</span> 后点
                <span class="text-foreground">设置</span> 可单独覆盖。
                当前会话模型：
                <span class="font-mono text-accent">{{ s.settings.model }}</span>
                （创造性 {{ s.effectiveTemperature }}，最大输出 {{ s.effectiveMaxTokens }} tokens）。
              </template>
              <template v-else>
                模型与 API 凭据由平台账户登录后自动注入；内置千问/深度求索参数使用应用默认。
                当前会话模型：
                <span class="font-mono text-accent">{{ s.settings.model }}</span>
                （创造性 {{ s.effectiveTemperature }}，最大输出 {{ s.effectiveMaxTokens }} tokens）。
              </template>
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
                  class="h-8 px-4 rounded-lg bg-accent text-sm font-medium text-white hover:opacity-95 cursor-pointer transition-opacity disabled:opacity-50"
                  :disabled="platformAuth.loading"
                  @click="loginPlatformAccount"
                >
                  {{ platformAuth.loading ? '等待授权…' : '浏览器登录' }}
                </button>
                <button
                  v-if="platformAuth.loading"
                  type="button"
                  class="h-8 px-4 rounded-lg border border-border text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
                  @click="platformAuth.cancelLogin()"
                >
                  取消
                </button>
              </div>
              <p
                v-if="platformAuth.error"
                class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
                role="alert"
              >
                {{ platformAuth.error }}
              </p>
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

          <!-- Provider panel stays mounted while debug menus are on (preserves in-progress edits). -->
          <section v-if="showDebugMenus" v-show="activeSection === 'provider'" class="p-6">
            <ProviderSettingsPanel ref="providerPanelRef" />
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
  </div>
</template>
