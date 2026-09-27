<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Check, ChevronRight, Cloud, Copy, Cpu, Plus, Trash2, Wrench, X } from 'lucide-vue-next'
import type { SettingsDialogForm } from '../../composables/useSettingsDialogForm'
import type { ModelRuntimeOverrides, ProviderConfig } from '../../types/chat'
import {
  detectProviderTemplateId,
  PROVIDER_TEMPLATE_OPTIONS,
  providerDraftForTemplate,
  providerTemplateMeta,
  stripProviderExtensionFields,
  type ProviderTemplateId
} from '../../lib/providerParams'
import {
  buildCustomModelEntryFromProvider,
  DEFAULT_CONTEXT_BUDGET_TOKENS,
  DEFAULT_MODEL_MAX_TOKENS,
  DEFAULT_MODEL_TEMPERATURE,
  patchProviderModelCapability,
  pruneInheritedModelConfigs,
  sanitizeProviderModelConfigs,
  useRuntimeParams
} from '../../composables/useRuntimeParams'
import { resolvedModelCapabilities } from '../../lib/modelCapabilities'
import RuntimeParamsForm from './RuntimeParamsForm.vue'
import ModelCapabilityForm from './ModelCapabilityForm.vue'
import { useSettingsStore } from '../../stores/settings'
import { usePlatformAuthStore } from '../../stores/platformAuth'

defineProps<{
  form: SettingsDialogForm
}>()

const { t } = useI18n()
const s = useSettingsStore()
const platformAuth = usePlatformAuthStore()
const isStandalone = computed(() => platformAuth.isStandalone)
// 分组只看 source：platform 归「平台服务」，其余（含缺 source、用户 fork）为自定义。
const isPlatformProvider = (provider: ProviderConfig) => provider.source === 'platform'
const platformProviders = computed(() =>
  isStandalone.value ? [] : s.settings.providers.filter(isPlatformProvider)
)
const customProviders = computed(() =>
  isStandalone.value
    ? s.settings.providers
    : s.settings.providers.filter(provider => !isPlatformProvider(provider))
)
const editableTemplateOptions = computed(() => {
  if (showAddProvider.value) {
    // 添加服务：已有服务商 id（含平台注入）不要再用同 id 模板自建。
    const occupiedIds = new Set(s.settings.providers.map(p => p.id))
    return PROVIDER_TEMPLATE_OPTIONS.filter(
      option => !option.defaultId || !occupiedIds.has(option.defaultId)
    )
  }
  // 编辑已有服务：类型锁定为当前服务的类型，只显示一个按钮，
  // 避免把所有模板（看起来像所有 provider）都列出来。
  return PROVIDER_TEMPLATE_OPTIONS.filter(option => option.id === providerTemplate.value)
})

const copiedKey = ref(false)
const editingProvider = ref<ProviderConfig | null>(null)
const pendingProviderDeletion = ref<ProviderConfig | null>(null)
const showAddProvider = ref(false)
const editingModelsText = ref('')
const originalApiKey = ref('')
const editingApiKey = ref('')

const modelConfigModalId = ref<string | null>(null)
const modelConfigModalError = ref('')
const providerSaveError = ref('')
const providerScopeModelId = ref<string | null>(null)
const providerTemplate = ref<ProviderTemplateId>('openai_compatible')

const globalGenFallback = {
  temperature: () => s.settings.temperature,
  maxTokens: () => s.settings.maxTokens,
  contextBudgetTokens: () => s.settings.contextBudgetTokens
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

function maskKey(key: unknown): string {
  if (typeof key !== 'string' || !key) return ''
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
  if (showAddProvider.value) return t('settings.models.service.apiKeyPlaceholderAdd')
  return t('settings.models.service.apiKeyPlaceholderEdit')
})

function providerKeyDisplay(key: unknown): string {
  return typeof key === 'string' && key ? maskKey(key) : t('settings.models.service.notConfigured')
}

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
  return p.modelConfigs?.[modelId] ? 'custom' : 'same'
}

function modelCapabilitySummary(modelId: string): string {
  const p = editingProvider.value
  if (!p) return ''
  const caps = resolvedModelCapabilities([p], p.id, modelId)
  const parts: string[] = []
  if (caps.supportsVision) parts.push(t('settings.models.service.capability.vision'))
  if (caps.supportsAudio) parts.push(t('settings.models.service.capability.audio'))
  if (caps.canGenerateImage) parts.push(t('settings.models.service.capability.imageGen'))
  if (caps.canGenerateVideo) parts.push(t('settings.models.service.capability.videoGen'))
  return parts.join(' · ')
}

function patchEditingModelCapability(
  flag: 'supportsVision' | 'supportsAudio' | 'canGenerateImage' | 'canGenerateVideo',
  value: boolean
) {
  const p = editingProvider.value
  const modelId = modelConfigModalId.value
  if (!p || !modelId) return
  // Replace the draft object so Vue/Pinia see the modelConfigs change (do not mutate props).
  editingProvider.value = patchProviderModelCapability(p, modelId, flag, value)
}

function setModelConfigMode(modelId: string, mode: 'same' | 'custom') {
  if (!editingProvider.value) return
  if (mode === 'same') {
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
  if (!editingProvider.value) return
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

function globalGenDefaults() {
  const t = s.settings.temperature
  const n = s.settings.maxTokens
  return {
    temperature: Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE,
    maxTokens: n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS,
    // New custom providers always start at 256K; do not copy a leftover global.
    contextBudgetTokens: DEFAULT_CONTEXT_BUDGET_TOKENS
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

function canEditProvider(provider: ProviderConfig) {
  return !isPlatformProvider(provider) || isStandalone.value
}

function startEditProvider(provider: ProviderConfig) {
  if (!canEditProvider(provider)) return
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
    // 只有用户本次显式输入的 key 才提交。未编辑 key 时提交空串：
    // 后端 update_user_settings 会用内存里的 key 回填（空则保持空）。
    // 平台注入的 key（OAuth / 登录）不进入 user 层，不会落盘。
    apiKey: editingApiKey.value ? editingApiKey.value : '',
    // 编辑保存 = 用户接管该 provider：无论原来来自哪层，保存后都属于
    // user 层（platform 注入项编辑保存 = fork 到 user 层）。
    source: 'user',
    modelConfigs: { ...(draft.modelConfigs ?? {}) }
  }

  if (snapshot.enableThinking !== true) {
    delete snapshot.thinkingBudget
  }
  // Keep thinkingIntensity / thinkingProtocol / reasoningEffort for strategy translation.

  const nextMc = sanitizeProviderModelConfigs(
    snapshot,
    models,
    snapshot.modelConfigs,
    globalGenFallback
  )
  snapshot.modelConfigs = nextMc
  return snapshot
}

function applyProviderSnapshotToStore(
  snapshot: ProviderConfig,
  wasAdd: boolean,
  reopenEdit = true
): boolean {
  const id = snapshot.id.trim()
  if (wasAdd && s.settings.providers.some(p => p.id === id)) {
    providerSaveError.value = t('settings.models.service.errorIdExists')
    return false
  }
  if (!wasAdd && !s.settings.providers.some(p => p.id === id)) {
    providerSaveError.value = t('settings.models.service.errorProviderNotFound')
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

function flushEditingProviderToStore(reopenEdit = false): boolean {
  if (!editingProvider.value) return true
  const snapshot = buildProviderSnapshotFromEditor()
  if (!snapshot) {
    providerSaveError.value = t('settings.models.service.errorMissingFields')
    return false
  }
  return applyProviderSnapshotToStore(snapshot, showAddProvider.value, reopenEdit)
}

async function saveProvider() {
  if (
    editingProvider.value
    && !showAddProvider.value
    && !canEditProvider(editingProvider.value)
  ) return
  providerSaveError.value = ''
  const snapshot = buildProviderSnapshotFromEditor()
  if (!snapshot) {
    providerSaveError.value = t('settings.models.service.errorMissingFields')
    return
  }

  const wasAdd = showAddProvider.value
  // Add / Save → collapse to the list; reopen via the wrench if more edits are needed.
  if (!applyProviderSnapshotToStore(snapshot, wasAdd, false)) return

  try {
    // 提交 providers 时，只有本次编辑的 provider 保留显式输入的 key；
    // 其余统一置空，由后端用「内存里的 key」回填，避免把平台注入的 key
    // （OAuth / 登录）误存进 user 层。
    // source 标记：编辑项已是 'user'（fork），非编辑的平台注入项标记
    // 'platform'，后端据此过滤不落盘。
    const providersForSave = s.settings.providers.map(p => ({
      ...p,
      apiKey: p.id === snapshot.id ? p.apiKey : '',
      source: p.id === snapshot.id ? 'user' : (p.source ?? 'user')
    }))
    await s.saveModelService({
      providers: providersForSave,
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model,
      temperature: s.settings.temperature,
      maxTokens: s.settings.maxTokens
    })
    providerSaveError.value = ''
  } catch (e) {
    console.error('[settings] save provider failed', e)
    providerSaveError.value = t('settings.models.service.errorApplyFailed')
  }
}

function requestCustomProviderDeletion(provider: ProviderConfig) {
  if (isPlatformProvider(provider)) return
  pendingProviderDeletion.value = provider
}

function cancelCustomProviderDeletion() {
  pendingProviderDeletion.value = null
}

async function confirmCustomProviderDeletion() {
  const provider = pendingProviderDeletion.value
  if (!provider) return
  pendingProviderDeletion.value = null
  await removeCustomProvider(provider)
}

async function removeCustomProvider(provider: ProviderConfig) {
  if (isPlatformProvider(provider)) return

  const id = provider.id
  const before = {
    providers: s.settings.providers,
    activeProviderId: s.settings.activeProviderId,
    model: s.settings.model
  }
  s.removeProvider(id)
  if (s.settings.providers.some(entry => entry.id === id)) return

  try {
    // 删除仅适用于自定义服务。提交时清空其他服务的显式 key，
    // 由后端以内存 key 池回填，避免把平台注入密钥写入 user 层。
    await s.saveModelService({
      providers: s.settings.providers.map(entry => ({
        ...entry,
        apiKey: '',
        source: isPlatformProvider(entry) ? 'platform' : 'user'
      })),
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model,
      temperature: s.settings.temperature,
      maxTokens: s.settings.maxTokens
    })
    providerSaveError.value = ''
  } catch (e) {
    console.error('[settings] remove provider failed', e)
    s.settings.providers = before.providers
    s.settings.activeProviderId = before.activeProviderId
    s.settings.model = before.model
    providerSaveError.value = t('settings.models.service.errorDeleteFailed')
    return
  }

  if (editingProvider.value?.id === id) {
    editingProvider.value = null
    showAddProvider.value = false
    modelConfigModalId.value = null
    modelConfigModalError.value = ''
  }
}

function hasUnsavedEdits(): boolean {
  return editingProvider.value !== null
}

function isModelConfigOpen(): boolean {
  return modelConfigModalId.value !== null
}

defineExpose({
  flushEditingProviderToStore,
  hasUnsavedEdits,
  isModelConfigOpen,
  closeModelConfigModal
})
</script>

<template>
  <div class="space-y-4">
    <!-- 自定义服务（上）：自行接入的服务优先展示 -->
    <section class="rounded-xl border border-border bg-card p-5 space-y-3">
      <div class="flex items-center justify-between gap-3">
        <div>
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Wrench class="w-4 h-4 text-accent" />{{ t('settings.models.service.customServices') }}
          </h4>
          <p class="mt-1 text-[11px] text-muted">
            {{ isStandalone ? t('settings.models.service.customServicesHintStandalone') : t('settings.models.service.customServicesHintDefault') }}
          </p>
        </div>
        <button
          type="button"
          class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent/10 hover:bg-accent/20 text-[12px] text-accent cursor-pointer transition-colors shrink-0"
          @click="startAddProvider"
        >
          <Plus class="w-3.5 h-3.5" />
          {{ t('settings.models.service.addService') }}
        </button>
      </div>
      <div class="space-y-2">
        <div
          v-for="p in customProviders"
          :key="p.id"
          class="group relative rounded-xl border p-4 transition-all"
          :class="s.settings.activeProviderId === p.id ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
        >
          <div class="flex items-start gap-3">
            <div class="mt-0.5 w-2 h-2 rounded-full shrink-0" :class="p.apiKey ? 'bg-success' : 'bg-warning'" />
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 min-w-0">
                <span class="text-sm font-medium text-foreground truncate">{{ p.name }}</span>
                <span class="text-[11px] text-muted shrink-0">{{ (p.models?.length ?? 0) > 0 ? t('settings.models.service.modelsCountSuffix', { n: p.models!.length }) : t('settings.models.service.noModelsConfigured') }}</span>
                <span v-if="s.settings.activeProviderId === p.id" class="px-1.5 py-0.5 rounded bg-hover text-[10px] font-medium text-foreground shrink-0">{{ t('settings.models.service.defaultGlobalProvider') }}</span>
              </div>
              <p class="mt-0.5 text-[11px] text-muted truncate font-mono">{{ p.baseUrl }}</p>
              <p class="mt-1 text-[11px] text-muted">{{ t('settings.models.service.apiKeyPrefix', { key: providerKeyDisplay(p.apiKey) }) }}</p>
            </div>
            <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                type="button"
                class="p-1.5 rounded-lg hover:bg-hover cursor-pointer transition-colors"
                :title="t('settings.models.service.editProviderTitle', { name: p.name })"
                :aria-label="t('settings.models.service.editProviderTitle', { name: p.name })"
                @click="startEditProvider(p)"
              >
                <Wrench class="w-3.5 h-3.5 text-accent" />
              </button>
              <button
                v-if="!isPlatformProvider(p) || isStandalone"
                type="button"
                class="p-1.5 rounded-lg hover:bg-hover cursor-pointer transition-colors"
                :title="t('settings.models.service.deleteProviderTitle', { name: p.name })"
                :aria-label="t('settings.models.service.deleteProviderTitle', { name: p.name })"
                @click="requestCustomProviderDeletion(p)"
              >
                <Trash2 class="w-3.5 h-3.5 text-danger" />
              </button>
              <button
                v-if="s.settings.activeProviderId !== p.id"
                type="button"
                class="ml-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors"
                @click="s.setActiveProvider(p.id)"
              >
                {{ t('settings.models.service.setDefault') }}
              </button>
            </div>
          </div>
        </div>

        <p v-if="providerSaveError" class="text-[12px] text-danger" role="alert">{{ providerSaveError }}</p>
        <div v-if="customProviders.length === 0" class="rounded-xl border border-dashed border-border p-8 text-center">
          <Cpu class="w-8 h-8 text-muted/80 mx-auto mb-2" />
          <p class="text-sm text-muted">{{ t('settings.models.service.noCustomServices') }}</p>
          <p class="text-xs text-muted/80 mt-1">{{ t('settings.models.service.noCustomServicesHint') }}</p>
        </div>
      </div>
    </section>

    <!-- 平台服务（下）：连官网时只读。standalone 全部列在上方自定义服务。 -->
    <section v-if="!isStandalone" class="rounded-xl border border-border bg-card p-5 space-y-3">
      <div class="flex items-center justify-between gap-3">
        <div>
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Cloud class="w-4 h-4 text-accent" />{{ t('settings.models.service.platformServices') }}
          </h4>
          <p class="mt-1 text-[11px] text-muted">{{ t('settings.models.service.platformServicesHint') }}</p>
        </div>
        <span class="shrink-0 px-2 py-0.5 rounded-full bg-muted/50 text-[10px] text-muted font-medium">{{ t('settings.models.service.readOnly') }}</span>
      </div>
      <div class="space-y-2">
        <div
          v-for="p in platformProviders"
          :key="p.id"
          class="group relative rounded-xl border p-4 transition-all"
          :class="s.settings.activeProviderId === p.id ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
        >
          <div class="flex items-start gap-3">
            <div class="mt-0.5 w-2 h-2 rounded-full shrink-0 bg-accent/50" />
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 min-w-0">
                <span class="text-sm font-medium text-foreground truncate">{{ p.name }}</span>
                <span class="text-[11px] text-muted shrink-0">{{ (p.models?.length ?? 0) > 0 ? t('settings.models.service.modelsCountSuffix', { n: p.models!.length }) : t('settings.models.service.noModelsConfigured') }}</span>
                <span v-if="s.settings.activeProviderId === p.id" class="px-1.5 py-0.5 rounded bg-hover text-[10px] font-medium text-foreground shrink-0">{{ t('settings.models.service.defaultGlobalProvider') }}</span>
              </div>
              <p class="mt-0.5 text-[11px] text-muted truncate font-mono">{{ p.baseUrl }}</p>
            </div>
            <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                v-if="s.settings.activeProviderId !== p.id"
                type="button"
                class="h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors"
                @click="s.setActiveProvider(p.id)"
              >
                {{ t('settings.models.service.setDefault') }}
              </button>
            </div>
          </div>
        </div>
        <div v-if="platformProviders.length === 0" class="rounded-xl border border-dashed border-border p-8 text-center">
          <Cloud class="w-8 h-8 text-muted/80 mx-auto mb-2" />
          <p class="text-sm text-muted">{{ t('settings.models.service.noPlatformServices') }}</p>
          <p class="text-xs text-muted/80 mt-1">{{ t('settings.models.service.noPlatformServicesHint') }}</p>
        </div>
      </div>
    </section>

  <Teleport to="body">
    <div
      v-if="editingProvider"
      class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
      role="presentation"
      @click.self="cancelEditProvider"
    >
      <div class="w-full max-w-xl max-h-[85vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
        <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
          <div>
            <h4 class="text-sm font-semibold text-foreground flex items-center gap-2">
              <ChevronRight class="w-4 h-4 text-accent" />
              {{ showAddProvider ? t('settings.models.service.addModelServiceTitle') : t('settings.models.service.editModelServiceTitle') }}
            </h4>
            <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.models.service.modalHint') }}</p>
          </div>
          <button
            type="button"
            class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
            :aria-label="t('settings.models.service.close')"
            @click="cancelEditProvider"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
        <div class="p-5 space-y-4 overflow-y-auto">

      <div class="space-y-2">
        <label class="block text-[12px] text-muted">{{ t('settings.models.service.serviceType') }}</label>
        <div class="inline-flex flex-wrap gap-1 rounded-lg bg-card border border-border p-0.5">
          <button
            v-for="opt in editableTemplateOptions"
            :key="opt.id"
            type="button"
            class="h-8 px-3 rounded-md text-[12px] cursor-pointer transition-colors"
            :class="providerTemplate === opt.id ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
            :disabled="!showAddProvider"
            @click="setProviderTemplate(opt.id)"
          >
            {{ opt.label }}
          </button>
        </div>
        <p class="text-[11px] text-muted">
          {{ t('settings.models.service.templateHint') }}
          <span class="text-muted">（{{ providerTemplateHint }}）</span>
        </p>
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.models.service.serviceId') }}</label>
          <input v-model="editingProvider.id" :disabled="!showAddProvider" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 disabled:opacity-50 transition-colors" :placeholder="t('settings.models.service.serviceIdPlaceholder')" />
        </div>
        <div>
          <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.models.service.serviceName') }}</label>
          <input v-model="editingProvider.name" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" :placeholder="t('settings.models.service.serviceNamePlaceholder')" />
        </div>
        <div class="col-span-2">
          <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.models.service.apiUrl') }}</label>
          <input v-model="editingProvider.baseUrl" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" placeholder="https://api.example.com/v1" />
        </div>
        <div class="col-span-2">
          <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.models.service.apiKey') }}</label>
          <div class="flex items-center gap-2 h-9 px-3 rounded-lg bg-card border border-border transition-colors focus-within:border-primary/50">
            <input :value="displayKey" :type="editingApiKey || showAddProvider ? 'password' : 'text'" class="flex-1 bg-transparent border-0 outline-none text-sm text-foreground placeholder:text-muted font-mono" :placeholder="inputPlaceholder" @focus="clearMaskedInput" @input="e => { editingApiKey = (e.target as HTMLInputElement).value }" />
            <button v-if="!showAddProvider && originalApiKey" type="button" class="p-1 rounded hover:bg-hover cursor-pointer transition" :class="copiedKey ? 'text-success' : 'text-muted hover:text-foreground'" :title="copiedKey ? t('settings.models.service.copied') : t('settings.models.service.copyOriginalKey')" @click="copyOriginalKey">
              <Check v-if="copiedKey" class="w-4 h-4" />
              <Copy v-else class="w-4 h-4" />
            </button>
          </div>
        </div>
        <div class="col-span-2">
          <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.models.service.modelList') }}</label>
          <input v-model="editingModelsText" type="text" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" placeholder="model-1, model-2, model-3" />
        </div>
        <div class="col-span-2 rounded-lg border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3">
          <h5 class="text-[12px] font-medium text-foreground">{{ t('settings.models.service.modelParams') }}</h5>
          <p class="text-[11px] text-muted">{{ t('settings.models.service.modelParamsHint') }}</p>
          <RuntimeParamsForm :api="providerRuntimeApi" />
          <div v-if="editingParsedModelIds.length" class="pt-2 border-t border-border space-y-1.5">
            <div class="text-[11px] text-muted">{{ t('settings.models.service.perModel') }}</div>
            <ul class="rounded-lg border border-border bg-hover divide-y divide-border overflow-hidden">
              <li
                v-for="mid in editingParsedModelIds"
                :key="mid"
                class="flex items-center gap-2 px-3 py-2 min-h-10"
              >
                <div class="flex-1 min-w-0">
                  <span class="block font-mono text-[12px] text-foreground truncate" :title="mid">{{ mid }}</span>
                  <span
                    v-if="modelConfigMode(mid) === 'custom'"
                    class="block text-[10px] text-accent truncate mt-0.5"
                  >{{ t('settings.models.service.customized') }}<template v-if="modelCapabilitySummary(mid)"> · {{ modelCapabilitySummary(mid) }}</template></span>
                  <span v-else class="block text-[10px] text-muted truncate mt-0.5">{{ t('settings.models.service.followsProviderDefault') }}</span>
                </div>
                <button
                  type="button"
                  class="shrink-0 h-7 px-3 rounded-lg text-[11px] cursor-pointer transition-colors"
                  :class="modelConfigMode(mid) === 'custom' ? 'bg-hover text-foreground' : 'bg-hover/60 text-muted hover:bg-hover hover:text-foreground'"
                  @click="openModelConfigModal(mid)"
                >
                  {{ modelConfigMode(mid) === 'custom' ? t('settings.models.service.edit') : t('settings.models.service.configure') }}
                </button>
              </li>
            </ul>
          </div>
        </div>
      </div>

        </div>
        <div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3 shrink-0">
          <p v-if="providerSaveError" class="mr-auto text-[12px] text-danger">{{ providerSaveError }}</p>
          <button type="button" class="h-8 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="cancelEditProvider">{{ t('settings.models.service.cancel') }}</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity" :disabled="!editingProvider.id?.trim() || !editingProvider.name?.trim() || !editingProvider.baseUrl?.trim()" @click="saveProvider">
            {{ showAddProvider ? t('settings.models.service.addAction') : t('settings.models.service.save') }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
  </div>

  <Teleport to="body">
    <div
      v-if="pendingProviderDeletion"
      class="pointer-events-auto fixed inset-0 z-[10003] flex items-center justify-center bg-foreground/32 p-4"
      role="presentation"
      @click.self="cancelCustomProviderDeletion"
    >
      <div class="w-full max-w-md rounded-xl border border-border bg-card shadow-2xl p-5 space-y-4" role="alertdialog" aria-modal="true" aria-labelledby="delete-provider-title" @click.stop>
        <div class="space-y-1">
          <h5 id="delete-provider-title" class="text-sm font-semibold text-foreground">{{ t('settings.models.service.confirmDeleteProviderTitle') }}</h5>
          <p class="text-sm text-muted">{{ t('settings.models.service.confirmDeleteProviderBody', { name: pendingProviderDeletion.name }) }}</p>
        </div>
        <div class="flex items-center justify-end gap-2">
          <button type="button" class="h-8 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="cancelCustomProviderDeletion">{{ t('settings.models.service.cancel') }}</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-danger text-white text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity" @click="confirmCustomProviderDeletion">{{ t('settings.models.service.confirmDelete') }}</button>
        </div>
      </div>
    </div>
  </Teleport>

  <Teleport to="body">
    <div
      v-if="modelConfigModalId && editingProvider"
      class="pointer-events-auto fixed inset-0 z-[10003] flex items-center justify-center bg-foreground/32 p-4"
      role="presentation"
      @click.self="closeModelConfigModal"
    >
      <div class="w-full max-w-md rounded-xl border border-border bg-card shadow-2xl p-4 space-y-3" @click.stop>
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <h5 class="text-sm font-medium text-foreground">{{ t('settings.models.service.modelParamsAndCapabilities') }}</h5>
            <p class="mt-0.5 text-[11px] text-muted font-mono truncate" :title="modelConfigModalId">{{ modelConfigModalId }}</p>
          </div>
          <button type="button" class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0" :aria-label="t('settings.models.service.close')" @click="closeModelConfigModal">
            <X class="w-4 h-4" />
          </button>
        </div>
        <ModelCapabilityForm
          v-if="modelConfigModalId && editingProvider"
          :provider="editingProvider"
          :model-id="modelConfigModalId"
          @patch="patchEditingModelCapability"
        />
        <RuntimeParamsForm v-if="modelConfigModalId" :api="modelRuntimeApi" />
        <div class="flex items-center justify-end gap-2 pt-1">
          <button
            v-if="modelConfigModalId && modelConfigMode(modelConfigModalId) === 'custom'"
            type="button"
            class="h-8 px-4 mr-auto rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors"
            @click="setModelConfigMode(modelConfigModalId, 'same')"
          >{{ t('settings.models.service.restoreDefault') }}</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-hover hover:bg-hover text-sm text-foreground cursor-pointer transition-colors" @click="closeModelConfigModal">{{ t('settings.models.service.cancel') }}</button>
          <button type="button" class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity" @click="confirmModelConfigModal">{{ t('settings.models.service.done') }}</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
