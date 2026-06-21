<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { SlidersHorizontal, Bug, X, Sun, Moon, Monitor, Sparkles, Bot, Cpu, Gauge, Database, MessageSquare, UserCircle, Cloud } from 'lucide-vue-next'
import { isTauriRuntime } from '../../lib/runtime'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { provideSettingsDialogForm } from '../../composables/useSettingsDialogForm'
import ProviderSettingsPanel from './ProviderSettingsPanel.vue'
import ChannelSettingsPanel from './ChannelSettingsPanel.vue'
import AssistantSettingsPanel from './panels/AssistantSettingsPanel.vue'
import GenerationSettingsPanel from './panels/GenerationSettingsPanel.vue'
import AgentSettingsPanel from './panels/AgentSettingsPanel.vue'
import AccountSettingsPanel from './panels/AccountSettingsPanel.vue'
import CloudSettingsPanel from './panels/CloudSettingsPanel.vue'
import RuntimeSettingsPanel from './panels/RuntimeSettingsPanel.vue'

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-skills'): void
}>()

const s = useSettingsStore()
const chat = useChatStore()

const activeSection = ref('assistant')
const saving = ref(false)

const alwaysSections = [
  { id: 'assistant', label: '智能体', desc: 'Computer 与工具权限', icon: Bot },
  { id: 'channels', label: 'IM 通道', desc: '微信/飞书/企微/钉钉', icon: MessageSquare }
] as const

const debugSections = [
  { id: 'provider', label: '模型服务', desc: '管理 AI 服务', icon: Cpu },
  { id: 'generation', label: '界面配置', desc: '界面', icon: Gauge },
  { id: 'agent', label: '智能模式', desc: '工作方式', icon: Gauge },
  { id: 'runtime', label: '运行时', desc: '存储与网络', icon: Database }
] as const

const debugSectionIds = new Set<string>(debugSections.map(item => item.id))
const persistedSectionIds = new Set<string>(['assistant', 'channels'])

const form = provideSettingsDialogForm({
  onClose: () => emit('close'),
  activeSection,
  debugSectionIds
})

const {
  theme,
  debugMenusEnabled,
  showDebugMenus,
  themeLabel,
  cycleTheme,
  currentThemeIcon,
  toggleDebugMenus,
  getAssistantSavePayload,
  getDebugSessionSavePayload,
  activeUiAgentId,
  initFormFromStore
} = form

const providerPanelRef = ref<InstanceType<typeof ProviderSettingsPanel> | null>(null)
const channelPanelRef = ref<InstanceType<typeof ChannelSettingsPanel> | null>(null)

const platformReadOnly = computed(() => !s.canEditPlatform)

const sections = computed(() => {
  const merged = showDebugMenus.value
    ? [...alwaysSections, ...debugSections]
    : [...alwaysSections]
  if (!isTauriRuntime()) return merged
  return [
    { id: 'account', label: '平台账户', desc: '登录与凭据', icon: UserCircle },
    { id: 'cloud', label: '云主机', desc: '购买与管理', icon: Cloud },
    ...merged
  ]
})

const isPersistedSection = computed(() => persistedSectionIds.has(activeSection.value))
const showFooterSave = computed(() => {
  if (activeSection.value === 'account' || activeSection.value === 'runtime' || activeSection.value === 'cloud') return false
  return (
    activeSection.value === 'assistant' ||
    activeSection.value === 'channels' ||
    debugSectionIds.has(activeSection.value)
  )
})
const footerSaveLabel = computed(() =>
  isPersistedSection.value ? '保存' : '保存(本次会话)'
)
const debugModeTitle = computed(() =>
  debugMenusEnabled.value ? '调试模式：已开启（点击关闭）' : '调试模式：已关闭（点击开启）'
)

onMounted(() => {
  initFormFromStore()
})

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
    } else if (activeSection.value === 'channels') {
      await channelPanelRef.value?.save()
    } else if (activeSection.value === 'assistant') {
      const payload = getAssistantSavePayload()
      await s.saveAgentPreferences({ ...payload, theme: s.settings.theme })
      await s.saveUser({
        computerAutoCompact: payload.computerAutoCompact,
        userCodingRules: payload.userCodingRules
      })
    } else {
      if (providerPanelRef.value?.hasUnsavedEdits() && !providerPanelRef.value.flushEditingProviderToStore()) {
        activeSection.value = 'provider'
        return
      }
      await s.save({
        ...getDebugSessionSavePayload(),
        theme: s.settings.theme
      })
    }
    await s.saveUser({ theme: s.settings.theme })
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="onDialogBackdropClick">
    <div
      class="w-[960px] max-w-[94vw] h-[740px] max-h-[90vh] glass-strong rounded-2xl border border-border shadow-2xl flex flex-col overflow-hidden"
      data-tauri-drag-region="false"
    >
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
        <main class="app-content-no-drag flex-1 overflow-y-auto" data-tauri-drag-region="false">
          <p
            v-if="platformReadOnly && activeSection !== 'account' && activeSection !== 'runtime'"
            class="mx-6 mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-muted"
          >
            仅平台管理员可修改平台配置；重启后恢复默认。登录后 API 密钥由平台自动注入。
          </p>
          <!-- ==================== Assistant Section ==================== -->
          <section v-if="activeSection === 'assistant'" class="p-6 space-y-5">
            <AssistantSettingsPanel :form="form" />
          </section>

          <section v-else-if="activeSection === 'channels'" class="p-6">
            <ChannelSettingsPanel ref="channelPanelRef" />
          </section>

          <!-- ==================== Generation Section ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 space-y-5">
            <GenerationSettingsPanel :form="form" />
          </section>

          <!-- ==================== Agent Section ==================== -->
          <section v-else-if="activeSection === 'agent'" class="p-6 space-y-5">
            <AgentSettingsPanel :form="form" />
          </section>

          <!-- ==================== Platform account (desktop) ==================== -->
          <section v-else-if="activeSection === 'account'" class="p-6 space-y-5">
            <AccountSettingsPanel :form="form" />
          </section>

          <section v-else-if="activeSection === 'cloud'" class="p-6 space-y-5">
            <CloudSettingsPanel :form="form" />
          </section>

          <!-- ==================== Runtime Section ==================== -->
          <section v-else-if="activeSection === 'runtime'" class="p-6 space-y-5">
            <RuntimeSettingsPanel />
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
