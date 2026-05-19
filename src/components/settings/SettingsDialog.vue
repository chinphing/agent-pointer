<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  Bot,
  Check,
  ChevronRight,
  Copy,
  Cpu,
  Database,
  FolderOpen,
  Gauge,
  Network,
  Plus,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Users,
  Wrench,
  X
} from 'lucide-vue-next'
import type { AgentDef, ModelRuntimeOverrides, ProviderConfig } from '../../types/chat'
import { listAgents } from '../../lib/api'
import { isDeepSeekProvider, isQwenProvider } from '../../lib/providerParams'
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
import { useSettingsStore } from '../../stores/settings'

const emit = defineEmits<{ (e: 'close'): void }>()
const s = useSettingsStore()

const saving = ref(false)
const activeSection = ref('provider')
const copiedKey = ref(false)

const toolApprovalMode = ref<'auto' | 'manual'>('auto')
const agentMode = ref<'single' | 'supervisor'>('single')
const leadAgentId = ref('')
const workspaceRoot = ref('')
const contextCompressionEnabled = ref(true)
const contextBudgetChars = ref(120_000)
const contextKeepRecentUserTurns = ref(6)
const contextSummaryMaxTokens = ref(2048)
const maxToolRounds = ref(100)
const maxSubAgentToolRounds = ref(100)
const rawContentViewEnabled = ref(true)
const debugDumpLlmPrompts = ref(false)
const agentTaskBoardHistoryTrim = ref<Record<string, boolean>>({})
const computerHumanLike = ref(false)
const agents = ref<AgentDef[]>([])

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

const globalGenFallback = {
  temperature: () => s.settings.temperature,
  maxTokens: () => s.settings.maxTokens
}

const providerRuntimeApi = useRuntimeParams(editingProvider, providerScopeModelId, globalGenFallback)
const modelRuntimeApi = useRuntimeParams(editingProvider, modelConfigModalId, globalGenFallback)

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
  const o = p.modelConfigs?.[modelId]
  if (!o) return 'same'
  return hasEffectiveModelOverride(o, p, globalGenFallback) ? 'custom' : 'same'
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

const sections = [
  { id: 'provider', label: '模型服务', desc: '管理 AI 服务', icon: Cpu },
  { id: 'generation', label: '生成参数', desc: '输出控制', icon: Gauge },
  { id: 'agent', label: '智能模式', desc: '工作方式', icon: Bot },
  { id: 'runtime', label: '运行时', desc: '存储与网络', icon: Database }
]

const workers = computed(() => agents.value.filter(a => a.role === 'worker' && a.enabled))

const supervisorAgent = computed(
  () =>
    agents.value.find(a => a.id === 'supervisor' && a.enabled) ||
    agents.value.find(a => a.role === 'supervisor')
)

function isCoderAgent(_a: AgentDef): boolean {
  return true
}

const workspaceDirName = computed(() => {
  const p = workspaceRoot.value
  if (!p) return ''
  return p.replace(/[/\\]+$/, '').split(/[/\\]/).pop() || ''
})

const workspaceRequired = computed(
  () => agentMode.value === 'single' && workers.value.some(w => w.id === leadAgentId.value && isCoderAgent(w))
)

async function loadAgents() {
  try {
    agents.value = await listAgents()
  } catch (e) {
    console.error(e)
  }
}

async function pickWorkspace() {
  if (!isTauriRuntime()) return
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const dir = await open({ directory: true, multiple: false })
    if (typeof dir === 'string' && dir) workspaceRoot.value = dir
  } catch (e) {
    console.error(e)
  }
}

onMounted(() => {
  toolApprovalMode.value = s.settings.toolApprovalMode || 'auto'
  agentMode.value = s.settings.agentMode || 'single'
  leadAgentId.value = s.settings.leadAgentId || 'default'
  workspaceRoot.value = s.settings.workspaceRoot || ''
  contextCompressionEnabled.value = s.settings.contextCompressionEnabled !== false
  contextBudgetChars.value = s.settings.contextBudgetChars ?? 120_000
  contextKeepRecentUserTurns.value = s.settings.contextKeepRecentUserTurns ?? 6
  contextSummaryMaxTokens.value = s.settings.contextSummaryMaxTokens ?? 2048
  maxToolRounds.value = s.settings.maxToolRounds ?? 100
  maxSubAgentToolRounds.value = s.settings.maxSubAgentToolRounds ?? s.settings.maxToolRounds ?? 100
  rawContentViewEnabled.value = s.settings.rawContentViewEnabled !== false
  debugDumpLlmPrompts.value = s.settings.debugDumpLlmPrompts === true
  agentTaskBoardHistoryTrim.value = { ...(s.settings.agentTaskBoardHistoryTrim ?? {}) }
  computerHumanLike.value = s.settings.computerHumanLike === true
  loadAgents()
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

function startEditProvider(provider: ProviderConfig) {
  // 从 store 拷贝草稿，勿把列表里的 provider 对象直接赋给 editingProvider（保存时会互相覆盖）。
  const pruned = pruneInheritedModelConfigs(provider, provider.modelConfigs, globalGenFallback)
  editingProvider.value = {
    ...provider,
    models: [...(provider.models ?? [])],
    modelConfigs: cloneModelConfigs(pruned)
  }
  originalApiKey.value = provider.apiKey
  editingApiKey.value = ''
  editingModelsText.value = (provider.models ?? []).join(', ')
  showAddProvider.value = false
}

function startAddProvider() {
  const t = s.settings.temperature
  const n = s.settings.maxTokens
  editingProvider.value = {
    id: '',
    name: '',
    baseUrl: '',
    apiKey: '',
    models: [],
    reasoningInMessages: true,
    temperature: Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE,
    maxTokens: n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS,
    modelConfigs: {}
  }
  originalApiKey.value = ''
  editingApiKey.value = ''
  editingModelsText.value = ''
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
    await s.save({
      providers: s.settings.providers,
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model
    })
    providerSaveError.value = ''
  } catch (e) {
    console.error(e)
    providerSaveError.value = '保存到本地失败，请重试'
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

async function saveAll() {
  saving.value = true
  providerSaveError.value = ''
  try {
    // 底部「保存配置」须先合并正在编辑的服务商（含新加的模型名），否则只保存了旧列表。
    if (editingProvider.value && !flushEditingProviderToStore()) {
      activeSection.value = 'provider'
      return
    }
    await s.save({
      providers: s.settings.providers,
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model,
      toolApprovalMode: toolApprovalMode.value,
      agentMode: agentMode.value,
      leadAgentId: agentMode.value === 'supervisor' ? '' : leadAgentId.value,
      workspaceRoot: workspaceRoot.value,
      contextCompressionEnabled: contextCompressionEnabled.value,
      contextBudgetChars: Number(contextBudgetChars.value),
      contextKeepRecentUserTurns: Number(contextKeepRecentUserTurns.value),
      contextSummaryMaxTokens: Number(contextSummaryMaxTokens.value),
      maxToolRounds: Number(maxToolRounds.value),
      maxSubAgentToolRounds: Number(maxSubAgentToolRounds.value),
      rawContentViewEnabled: rawContentViewEnabled.value,
      debugDumpLlmPrompts: debugDumpLlmPrompts.value,
      agentTaskBoardHistoryTrim: { ...agentTaskBoardHistoryTrim.value },
      computerHumanLike: computerHumanLike.value
    })
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="modelConfigModalId ? closeModelConfigModal() : emit('close')">
    <div class="w-[960px] max-w-[94vw] h-[740px] max-h-[90vh] glass-strong rounded-2xl border border-white/10 shadow-2xl flex flex-col overflow-hidden">
      <!-- Header -->
      <header class="px-6 h-14 flex items-center gap-3 border-b border-white/5 shrink-0">
        <div class="w-8 h-8 rounded-lg bg-gradient-to-br from-primary/30 to-primary-fuchsia/30 flex items-center justify-center">
          <SlidersHorizontal class="w-4 h-4 text-primary-cyan" />
        </div>
        <div>
          <h2 class="text-base font-semibold text-slate-100">设置</h2>
          <p class="text-[11px] text-slate-500">配置 AI 模型、生成参数和工作模式</p>
        </div>
        <div class="flex-1" />
        <button class="p-2 rounded-lg hover:bg-white/5 cursor-pointer transition-colors" @click="emit('close')">
          <X class="w-4 h-4 text-slate-400" />
        </button>
      </header>

      <div class="flex flex-1 min-h-0">
        <!-- Sidebar -->
        <aside class="w-56 shrink-0 border-r border-white/5 p-3 bg-black/10">
          <button
            v-for="item in sections"
            :key="item.id"
            class="w-full flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-all cursor-pointer group"
            :class="activeSection === item.id ? 'bg-primary/15 border border-primary/30' : 'border border-transparent hover:bg-white/[0.05]'"
            @click="activeSection = item.id"
          >
            <div class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors"
                 :class="activeSection === item.id ? 'bg-primary/20' : 'bg-white/5 group-hover:bg-white/10'">
              <component :is="item.icon" class="w-3.5 h-3.5" :class="activeSection === item.id ? 'text-primary-cyan' : 'text-slate-400'" />
            </div>
            <span class="min-w-0">
              <span class="block text-[13px] font-medium" :class="activeSection === item.id ? 'text-slate-100' : 'text-slate-300'">{{ item.label }}</span>
              <span class="block text-[11px] text-slate-500 truncate">{{ item.desc }}</span>
            </span>
          </button>
        </aside>

        <!-- Main Content -->
        <main class="flex-1 overflow-y-auto">
          <!-- ==================== Provider Section ==================== -->
          <section v-if="activeSection === 'provider'" class="p-6 space-y-5">
            <div class="flex items-center justify-between">
              <div>
                <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                  <Cpu class="w-4 h-4 text-primary-cyan" />模型服务
                </h3>
                <p class="mt-0.5 text-xs text-slate-500">管理 AI 模型服务的连接配置</p>
              </div>
              <button
                class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-primary/15 hover:bg-primary/25 text-[12px] text-primary-cyan cursor-pointer transition-colors"
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
                :class="s.settings.activeProviderId === p.id ? 'border-primary/40 bg-primary/5' : 'border-white/5 bg-black/20 hover:border-white/10'"
              >
                <div class="flex items-start gap-3">
                  <!-- Status indicator -->
                  <div class="mt-0.5 w-2 h-2 rounded-full shrink-0" :class="p.apiKey ? 'bg-green-400' : 'bg-amber-400'" />
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-slate-100">{{ p.name }}</span>
                      <span v-if="s.settings.activeProviderId === p.id" class="px-1.5 py-0.5 rounded bg-primary/20 text-[10px] font-medium text-primary-cyan">使用中</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-slate-500 truncate font-mono">{{ p.baseUrl }}</p>
                    <div class="mt-1 flex items-center gap-3 text-[11px] text-slate-500">
                      <span>密钥：{{ providerKeyDisplay(p.apiKey) }}</span>
                      <span class="text-white/10">|</span>
                      <!-- models 须 optional chain：normalize 前旧数据可能缺该字段，直接 .length 会导致整页白屏 -->
                      <span>模型：{{ (p.models?.length ?? 0) > 0 ? `${p.models!.length} 个` : '未配置' }}</span>
                    </div>
                  </div>
                  <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button class="p-1.5 rounded-lg hover:bg-white/10 cursor-pointer transition-colors" @click="startEditProvider(p)">
                      <Wrench class="w-3.5 h-3.5 text-slate-400" />
                    </button>
                    <button class="p-1.5 rounded-lg hover:bg-white/10 cursor-pointer transition-colors" @click="removeProvider(p.id)">
                      <Trash2 class="w-3.5 h-3.5 text-danger" />
                    </button>
                    <button v-if="s.settings.activeProviderId !== p.id" class="ml-1 h-7 px-2.5 rounded-lg bg-primary/15 text-[11px] font-medium text-primary-cyan hover:bg-primary/25 cursor-pointer transition-colors" @click="s.setActiveProvider(p.id)">
                      使用
                    </button>
                  </div>
                </div>
              </div>

              <!-- Empty state -->
              <div v-if="s.settings.providers.length === 0" class="rounded-xl border border-dashed border-white/10 p-8 text-center">
                <Cpu class="w-8 h-8 text-slate-600 mx-auto mb-2" />
                <p class="text-sm text-slate-500">暂无模型服务</p>
                <p class="text-xs text-slate-600 mt-1">点击上方"添加服务"开始配置</p>
              </div>
            </div>

            <!-- Edit/Add Form -->
            <div v-if="editingProvider" class="rounded-xl border border-primary/30 bg-primary/5 p-5 space-y-4">
              <h4 class="text-sm font-medium text-slate-100 flex items-center gap-2">
                <ChevronRight class="w-4 h-4 text-primary-cyan" />
                {{ showAddProvider ? '添加模型服务' : '编辑模型服务' }}
              </h4>

              <div class="grid grid-cols-2 gap-3">
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">服务 ID</label>
                  <input v-model="editingProvider.id" :disabled="!showAddProvider" type="text" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 disabled:opacity-50 transition-colors" placeholder="例如：qwen, openai" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">服务名称</label>
                  <input v-model="editingProvider.name" type="text" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" placeholder="例如：千问" />
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-slate-400 mb-1.5">API 地址</label>
                  <input v-model="editingProvider.baseUrl" type="text" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" placeholder="https://api.example.com/v1" />
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-slate-400 mb-1.5">API 密钥</label>
                  <div class="flex items-center gap-2 h-9 px-3 rounded-lg bg-black/30 border border-white/10 transition-colors focus-within:border-primary/50">
                    <input :value="displayKey" @focus="clearMaskedInput" @input="e => { editingApiKey = (e.target as HTMLInputElement).value }" :type="editingApiKey || showAddProvider ? 'password' : 'text'" class="flex-1 bg-transparent border-0 outline-none text-sm text-slate-100 placeholder:text-slate-500 font-mono" :placeholder="inputPlaceholder" />
                    <button v-if="!showAddProvider && originalApiKey" class="p-1 rounded hover:bg-white/10 cursor-pointer transition" :class="copiedKey ? 'text-green-400' : 'text-slate-400 hover:text-slate-200'" @click="copyOriginalKey" :title="copiedKey ? '已复制' : '复制原始密钥'">
                      <Check v-if="copiedKey" class="w-4 h-4" />
                      <Copy v-else class="w-4 h-4" />
                    </button>
                  </div>
                </div>
                <div class="col-span-2">
                  <label class="block text-[12px] text-slate-400 mb-1.5">模型列表</label>
                  <input v-model="editingModelsText" type="text" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" placeholder="model-1, model-2, model-3"  />
                </div>
                <div class="col-span-2 rounded-lg border border-white/5 bg-black/20 p-4 space-y-3">
                  <h5 class="text-[12px] font-medium text-slate-200">模型参数</h5>
                  <p class="text-[11px] text-slate-500">服务商级默认；各模型可选「同上」或「定制」。</p>
                  <RuntimeParamsForm :api="providerRuntimeApi" />
                  <div v-if="editingParsedModelIds.length" class="pt-2 border-t border-white/5 space-y-1.5">
                    <div class="text-[11px] text-slate-500">各模型</div>
                    <ul class="rounded-lg border border-white/5 bg-black/15 divide-y divide-white/5 overflow-hidden">
                      <li
                        v-for="mid in editingParsedModelIds"
                        :key="mid"
                        class="flex items-center gap-2 px-3 py-2 min-h-10"
                      >
                        <span class="flex-1 min-w-0 font-mono text-[12px] text-slate-300 truncate" :title="mid">{{ mid }}</span>
                        <div class="inline-flex rounded-lg bg-black/30 border border-white/10 p-0.5 shrink-0">
                          <button
                            type="button"
                            class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
                            :class="modelConfigMode(mid) === 'same' ? 'bg-white/15 text-slate-100' : 'text-slate-500 hover:text-slate-300'"
                            @click="setModelConfigMode(mid, 'same')"
                          >同上</button>
                          <button
                            type="button"
                            class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
                            :class="modelConfigMode(mid) === 'custom' ? 'bg-white/15 text-slate-100' : 'text-slate-500 hover:text-slate-300'"
                            @click="setModelConfigMode(mid, 'custom')"
                          >定制</button>
                        </div>
                        <button
                          v-if="modelConfigMode(mid) === 'custom'"
                          type="button"
                          class="shrink-0 h-7 px-2.5 rounded-md bg-white/5 hover:bg-white/10 text-[11px] text-slate-200 cursor-pointer transition-colors"
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
                  <button class="h-8 px-4 rounded-lg bg-white/5 hover:bg-white/10 text-sm text-slate-300 cursor-pointer transition-colors" @click="cancelEditProvider">取消</button>
                  <button class="h-8 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity" :disabled="!editingProvider.id?.trim() || !editingProvider.name?.trim() || !editingProvider.baseUrl?.trim()" @click="saveProvider">
                    {{ showAddProvider ? '添加' : '保存' }}
                  </button>
                </div>
              </div>
            </div>
          </section>

          <!-- ==================== Generation Section ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Gauge class="w-4 h-4 text-primary-cyan" />生成参数
              </h3>
              <p class="mt-0.5 text-xs text-slate-500">创造性、最大输出等可在服务商级设默认，也可按模型定制</p>
            </div>

            <div class="rounded-xl border border-white/5 bg-black/20 p-4 text-[12px] text-slate-400 leading-relaxed">
              请在 <span class="text-slate-200">模型服务</span> 中编辑服务商，配置
              <span class="text-slate-200">创造性</span>、
              <span class="text-slate-200">最大输出</span> 等默认项；在「各模型」选择
              <span class="text-slate-200">定制</span> 后点
              <span class="text-slate-200">设置</span> 可单独覆盖。
              当前会话模型：
              <span class="font-mono text-primary-cyan">{{ s.settings.model }}</span>
              （创造性 {{ s.effectiveTemperature }}，最大输出 {{ s.effectiveMaxTokens }} tokens）。
            </div>

            <div class="rounded-xl border border-white/5 bg-black/20 p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-medium text-slate-100">原始内容查看</h4>
                  <p class="mt-1 text-[11px] text-slate-500">在助手消息上显示「原始输出」入口（代码图标），展开后为一段可复制文本：含推理（若有）与正文通道原始输出，不在主气泡内展示。</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="rawContentViewEnabled" type="checkbox" class="sr-only peer" />
                  <div class="w-9 h-5 bg-white/10 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary-cyan" />
                </label>
              </div>
            </div>

            <div class="rounded-xl border border-white/5 bg-black/20 p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-medium text-slate-100">保存每轮对话请求</h4>
                  <p class="mt-1 text-[11px] text-slate-500">开启后，每次向 AI 发送的完整上下文会分别保存为本地文件（应用数据目录下的日志文件夹），便于排查问题；内嵌的大块图片内容会缩短显示。</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="debugDumpLlmPrompts" type="checkbox" class="sr-only peer" />
                  <div class="w-9 h-5 bg-white/10 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary-cyan" />
                </label>
              </div>
            </div>

            <!-- Context Compression -->
            <div class="rounded-xl border border-white/5 bg-black/20 p-5 space-y-4">
              <div class="flex items-center justify-between">
                <h4 class="text-sm font-medium text-slate-100">上下文自动压缩</h4>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="contextCompressionEnabled" type="checkbox" class="sr-only peer" />
                  <div class="w-9 h-5 bg-white/10 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary-cyan"></div>
                </label>
              </div>
              <p class="text-[11px] text-slate-500">当历史消息超过预算时，自动生成摘要并保留最近若干轮对话原文。</p>

              <div v-if="contextCompressionEnabled" class="grid grid-cols-2 gap-3 pt-2 border-t border-white/5">
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">触发预算（字符）</label>
                  <input v-model.number="contextBudgetChars" type="number" min="8000" max="2000000" step="1000" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">保留最近用户轮数</label>
                  <input v-model.number="contextKeepRecentUserTurns" type="number" min="1" max="50" step="1" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">摘要最大 tokens</label>
                  <input v-model.number="contextSummaryMaxTokens" type="number" min="128" max="8192" step="64" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1.5">单轮最大工具调用轮次</label>
                  <input v-model.number="maxToolRounds" type="number" min="1" max="10000" step="1" class="w-full h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors" />
                </div>
              </div>
            </div>
          </section>

          <!-- ==================== Agent Section ==================== -->
          <section v-else-if="activeSection === 'agent'" class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Bot class="w-4 h-4 text-primary-cyan" />智能模式
              </h3>
              <p class="mt-0.5 text-xs text-slate-500">选择 AI 的工作方式和工具使用权限</p>
            </div>

            <!-- Agent Cards -->
            <div class="space-y-2">
              <h4 class="text-[12px] font-medium text-slate-400 uppercase tracking-wider">执行智能体</h4>

              <!-- Worker Agents -->
              <div
                v-for="w in workers"
                :key="w.id"
                class="rounded-xl border p-3 cursor-pointer transition-all"
                :class="agentMode === 'single' && (leadAgentId === w.id || ((!leadAgentId || leadAgentId === 'default') && w.id === 'default')) ? 'border-primary/30 bg-white/[0.03]' : 'border-white/5 bg-white/[0.02] hover:border-white/10'"
                @click="agentMode = 'single'; leadAgentId = w.id"
              >
                <div class="flex items-start gap-3">
                  <!-- Icon -->
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="agentMode === 'single' && (leadAgentId === w.id || ((!leadAgentId || leadAgentId === 'default') && w.id === 'default')) ? 'bg-primary/15' : 'bg-white/5'">
                    <Bot class="w-4 h-4" :class="agentMode === 'single' && (leadAgentId === w.id || ((!leadAgentId || leadAgentId === 'default') && w.id === 'default')) ? 'text-primary-cyan' : 'text-slate-400'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-slate-100">{{ w.name }}</span>
                      <span class="px-1.5 py-0.5 rounded bg-white/5 text-[10px] text-slate-500 font-mono">{{ w.id }}</span>
                      <span v-if="agentMode === 'single' && (leadAgentId === w.id || ((!leadAgentId || leadAgentId === 'default') && w.id === 'default'))" class="px-1.5 py-0.5 rounded bg-primary/20 text-[10px] font-medium text-primary-cyan">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-slate-500">{{ w.description || '通用智能体' }}</p>

                    <!-- Per-agent default model (lead or delegated sub-agent runs) -->
                    <div class="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-2" @click.stop>
                      <div class="flex items-center gap-2 min-w-0">
                        <Sparkles class="w-3.5 h-3.5 text-primary-fuchsia shrink-0" />
                        <span class="text-[11px] text-slate-400 shrink-0">默认模型</span>
                        <select
                          :value="getAgentModelWithProvider(w.id)"
                          @change.stop="selectAgentModelWithProvider(w.id, ($event.target as HTMLSelectElement).value)"
                          @click.stop
                          class="w-48 h-7 px-2 rounded bg-black/30 border border-white/10 text-[11px] text-slate-300 cursor-pointer outline-none focus:border-primary/50 transition-colors"
                        >
                          <option value="">使用全局默认</option>
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                      </div>

                      <label class="inline-flex items-center gap-1.5 cursor-pointer shrink-0">
                        <input
                          type="checkbox"
                          class="rounded border-white/20 bg-black/30 text-primary-cyan focus:ring-primary/40"
                          :checked="taskBoardTrimChecked(w.id)"
                          @change="setTaskBoardTrimLocal(w.id, ($event.target as HTMLInputElement).checked)"
                        />
                        <span class="text-[11px] text-slate-400">任务板后精简历史</span>
                      </label>

                      <label
                        v-if="w.id === 'computer'"
                        class="inline-flex items-center gap-1.5 cursor-pointer shrink-0"
                        title="启用后鼠标沿曲线移动并带微抖动；关闭时使用直线匀速移动（约 0.5 秒）"
                      >
                        <input
                          type="checkbox"
                          class="rounded border-white/20 bg-black/30 text-primary-cyan focus:ring-primary/40"
                          :checked="computerHumanLike"
                          @change="computerHumanLike = ($event.target as HTMLInputElement).checked"
                        />
                        <span class="text-[11px] text-slate-400">人性化鼠标移动</span>
                      </label>
                    </div>

                    <!-- Workspace (coder lead only) -->
                    <div v-if="agentMode === 'single' && leadAgentId === w.id && isCoderAgent(w)" class="mt-2 flex items-center gap-1.5">
                        <FolderOpen class="w-3.5 h-3.5 text-amber-300 shrink-0" />
                        <span
                          v-if="isTauriRuntime()"
                          class="text-[11px] text-slate-300 hover:text-primary-cyan cursor-pointer underline decoration-dashed underline-offset-2"
                          :title="workspaceRoot"
                          @click="pickWorkspace"
                        >{{ workspaceDirName || '选择工作目录…' }}</span>
                        <input
                          v-else
                          v-model="workspaceRoot"
                          type="text"
                          class="w-48 h-7 px-2 rounded bg-black/30 border border-white/10 text-[11px] text-slate-300 outline-none focus:border-primary/50 transition-colors"
                          placeholder="D:\project\my-repo"
                        />
                      </div>
                  </div>
                </div>
              </div>

              <!-- Supervisor Agent -->
              <div
                v-if="supervisorAgent"
                class="rounded-xl border p-3 cursor-pointer transition-all"
                :class="agentMode === 'supervisor' ? 'border-primary/30 bg-white/[0.03]' : 'border-white/5 bg-white/[0.02] hover:border-white/10'"
                @click="agentMode = 'supervisor'"
              >
                <div class="flex items-start gap-3">
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="agentMode === 'supervisor' ? 'bg-primary/15' : 'bg-white/5'">
                    <Users class="w-4 h-4" :class="agentMode === 'supervisor' ? 'text-primary-cyan' : 'text-slate-400'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-slate-100">{{ supervisorAgent.name }}</span>
                      <span v-if="agentMode === 'supervisor'" class="px-1.5 py-0.5 rounded bg-primary/20 text-[10px] font-medium text-primary-cyan">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-slate-500">多子智能体编排与结果整合</p>

                    <!-- Default Model Selector -->
                    <div v-if="agentMode === 'supervisor'" class="mt-2.5 flex items-center gap-2">
                      <Sparkles class="w-3.5 h-3.5 text-primary-fuchsia shrink-0" />
                      <span class="text-[11px] text-slate-400 shrink-0">默认模型</span>
                      <select
                        :value="getAgentModelWithProvider('supervisor')"
                        @change.stop="selectAgentModelWithProvider('supervisor', ($event.target as HTMLSelectElement).value)"
                        @click.stop
                        class="w-48 h-7 px-2 rounded bg-black/30 border border-white/10 text-[11px] text-slate-300 cursor-pointer outline-none focus:border-primary/50 transition-colors"
                      >
                        <option value="">使用全局默认</option>
                        <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div
              v-if="agentMode === 'single'"
              class="rounded-xl border border-white/5 bg-black/20 p-5 space-y-3"
            >
              <h4 class="text-sm font-medium text-slate-100">子任务委托</h4>
              <p class="text-[11px] text-slate-500">
                可委派的 worker 由主 Agent 的 AGENT.md 中 <code class="text-slate-400">allowAgents</code> 配置。
              </p>
              <div>
                <label class="block text-[12px] text-slate-400 mb-1.5">子 Agent 内工具轮次上限</label>
                <input
                  v-model.number="maxSubAgentToolRounds"
                  type="number"
                  min="1"
                  max="10000"
                  step="1"
                  class="w-full max-w-xs h-9 px-3 rounded-lg bg-black/30 border border-white/10 text-sm text-slate-100 outline-none focus:border-primary/50 transition-colors"
                />
              </div>
            </div>

            <!-- Tool Approval -->
            <div class="rounded-xl border border-white/5 bg-black/20 p-5 space-y-3">
              <h4 class="text-sm font-medium text-slate-100 flex items-center gap-2">
                <Wrench class="w-4 h-4 text-primary-fuchsia" />工具使用权限
              </h4>
              <div class="grid grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'auto' ? 'border-primary/40 bg-primary/5' : 'border-white/5 bg-black/20 hover:border-white/10'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-slate-100">自动执行</span>
                  <span class="mt-1 block text-[11px] text-slate-500">AI 使用工具时自动执行，无需确认</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'manual' ? 'border-primary/40 bg-primary/5' : 'border-white/5 bg-black/20 hover:border-white/10'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-slate-100">敏感操作确认</span>
                  <span class="mt-1 block text-[11px] text-slate-500">涉及文件、命令等操作时需要你确认</span>
                </label>
              </div>
            </div>
          </section>

          <!-- ==================== Runtime Section ==================== -->
          <section v-else class="p-6 space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Database class="w-4 h-4 text-primary-cyan" />运行时与存储
              </h3>
              <p class="mt-0.5 text-xs text-slate-500">查看当前存储与网络运行方式</p>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div class="rounded-xl border border-white/5 bg-black/20 p-5">
                <div class="w-9 h-9 rounded-lg bg-primary-fuchsia/10 flex items-center justify-center mb-3">
                  <Database class="w-4 h-4 text-primary-fuchsia" />
                </div>
                <div class="text-sm font-medium text-slate-100">本地数据</div>
                <p class="mt-1 text-xs text-slate-500">配置、API 密钥和会话记录保存在本机。</p>
              </div>
              <div class="rounded-xl border border-white/5 bg-black/20 p-5">
                <div class="w-9 h-9 rounded-lg bg-primary-fuchsia/10 flex items-center justify-center mb-3">
                  <Network class="w-4 h-4 text-primary-fuchsia" />
                </div>
                <div class="text-sm font-medium text-slate-100">网络</div>
                <p class="mt-1 text-xs text-slate-500">当前直接访问 AI 服务 API。</p>
              </div>
            </div>
          </section>
        </main>
      </div>

      <!-- Footer -->
      <footer class="px-6 h-14 flex items-center gap-3 border-t border-white/5 shrink-0">
        <p class="text-[11px] text-slate-500 flex-1">
          单智能体由所选 Worker 执行；Supervisor 为多任务编排。
        </p>
        <button class="h-9 px-4 rounded-lg bg-white/5 hover:bg-white/10 text-sm text-slate-300 cursor-pointer transition-colors" @click="emit('close')">取消</button>
        <button class="h-9 px-5 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity" :disabled="saving" @click="saveAll">
          {{ saving ? '保存中…' : '保存配置' }}
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
        <div class="w-full max-w-md rounded-xl border border-white/10 bg-[#12161c] shadow-2xl p-4 space-y-3" @click.stop>
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <h5 class="text-sm font-medium text-slate-100">模型参数</h5>
            <p class="mt-0.5 text-[11px] text-slate-500 font-mono truncate" :title="modelConfigModalId">{{ modelConfigModalId }}</p>
          </div>
          <button type="button" class="p-1.5 rounded-lg hover:bg-white/10 text-slate-400 cursor-pointer transition-colors shrink-0" aria-label="关闭" @click="closeModelConfigModal">
            <X class="w-4 h-4" />
          </button>
        </div>
        <RuntimeParamsForm v-if="modelConfigModalId" :api="modelRuntimeApi" />
        <div class="flex items-center justify-end gap-2 pt-1">
          <button type="button" class="h-8 px-4 rounded-lg bg-white/5 hover:bg-white/10 text-sm text-slate-300 cursor-pointer transition-colors" @click="closeModelConfigModal">取消</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity" @click="confirmModelConfigModal">完成</button>
        </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
