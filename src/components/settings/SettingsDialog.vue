<script setup lang="ts">
import { computed, nextTick, onErrorCaptured, onMounted, ref, watch } from 'vue'
import { ArrowLeft, Bug, Sparkles, Bot, Cpu, MessageSquare, Cloud, Clock, Info, Settings, Puzzle, Plug, Gauge } from 'lucide-vue-next'
import { isTauriRuntime } from '../../lib/runtime'
import { useWindowChrome } from '../../composables/useWindowChrome'
import WindowDragRegion from '../layout/WindowDragRegion.vue'
import WindowControls from '../layout/WindowControls.vue'
import { useSettingsStore } from '../../stores/settings'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { provideSettingsDialogForm } from '../../composables/useSettingsDialogForm'
import ChannelSettingsPanel from './ChannelSettingsPanel.vue'
import AssistantSettingsPanel from './panels/AssistantSettingsPanel.vue'
import GenerationSettingsPanel from './panels/GenerationSettingsPanel.vue'
import ModelSettingsPanel from './panels/ModelSettingsPanel.vue'
import DebugSettingsPanel from './panels/DebugSettingsPanel.vue'
import CloudSettingsPanel from './panels/CloudSettingsPanel.vue'
import AutomationSettingsPanel from './panels/AutomationSettingsPanel.vue'
import AboutSettingsPanel from './panels/AboutSettingsPanel.vue'
import UsageSettingsPanel from './panels/UsageSettingsPanel.vue'
import SkillsPanel from '../skills/SkillsPanel.vue'
import PluginsPanel from './panels/PluginsPanel.vue'
import McpPanel from './panels/McpPanel.vue'

const emit = defineEmits<{
  (e: 'close'): void
}>()
const props = withDefaults(defineProps<{
  initialSection?: string
}>(), {
  initialSection: 'assistant'
})

const s = useSettingsStore()
const platformAuth = usePlatformAuthStore()

const activeSection = ref(props.initialSection)
const mainEl = ref<HTMLElement | null>(null)
const renderError = ref('')
const {
  enabled: chromeEnabled,
  os,
  maximized,
  showCustomControls,
  macTrafficLightPadding,
  minimize,
  toggleMaximize,
  close: closeWindow
} = useWindowChrome()

const useWinLinuxWindowControls = computed(
  () => chromeEnabled && showCustomControls.value && (os.value === 'windows' || os.value === 'linux')
)

// 某个 section 渲染抛错时不再静默空白：显示错误条并阻断错误冒泡
// （否则整个 SettingsDialog 树可能白屏）。切到其他分区后错误条保留，
// 便于定位；切换即清空。
onErrorCaptured(err => {
  renderError.value = err instanceof Error ? err.message : String(err)
  console.error('[settings] section render error:', err)
  return false
})
// 系统设置页很长，切换分区时 main 滚动位置会残留；显式归零，
// 避免切到内容较短的分区时视觉上停在旧滚动位置。
watch(activeSection, () => {
  nextTick(() => {
    if (mainEl.value) mainEl.value.scrollTop = 0
  })
})

const alwaysSections = [
  { id: 'automation', label: '自动化', desc: '定时任务与 Webhook', icon: Clock },
  { id: 'channels', label: '连接', desc: '微信/飞书/企微/钉钉', icon: MessageSquare },
  { id: 'skills', label: '技能', desc: '启用与管理技能', icon: Sparkles },
  { id: 'plugins', label: '插件', desc: '管理 Pointer 插件', icon: Puzzle },
  { id: 'mcp', label: 'MCP', desc: '外部工具服务', icon: Plug },
  { id: 'assistant', label: '智能体', desc: '档位与行为', icon: Bot },
  { id: 'models', label: '模型配置', desc: '服务商与档位映射', icon: Cpu },
  { id: 'generation', label: '系统设置', desc: '界面、桌面与系统运行', icon: Settings }
] as const

const debugSections = [
  { id: 'debug', label: '调试', desc: '保存对话请求', icon: Bug }
] as const

const debugSectionIds = new Set<string>(debugSections.map(item => item.id))

const form = provideSettingsDialogForm({
  onClose: () => emit('close'),
  activeSection,
  debugSectionIds
})

const {
  initFormFromStore
} = form

const showAdminDebugSection = computed(() =>
  s.isPlatformAdmin || platformAuth.isPlatformAdmin || platformAuth.isStandalone
)

interface SidebarItem {
  id: string
  label: string
  desc: string
  icon: typeof Bot
}
interface SidebarGroup {
  items: SidebarItem[]
}

const sections = computed<SidebarGroup[]>(() => {
  const about: SidebarItem = {
    id: 'about',
    label: '关于',
    desc: '版本与更新',
    icon: Info
  }
  const usage: SidebarItem = {
    id: 'usage',
    label: '用量',
    desc: 'Token 消耗记录',
    icon: Gauge
  }

  const groups: SidebarGroup[] = [
    // 智能体与配置：决定 AI 怎么工作、怎么显示
    {
      items: [
        alwaysSections.find(item => item.id === 'assistant')!,
        alwaysSections.find(item => item.id === 'models')!,
        alwaysSections.find(item => item.id === 'generation')!
      ]
    },
    // 自动化与集成：外部接入
    {
      items: [
        alwaysSections.find(item => item.id === 'automation')!,
        alwaysSections.find(item => item.id === 'channels')!,
        alwaysSections.find(item => item.id === 'skills')!,
        alwaysSections.find(item => item.id === 'plugins')!,
        alwaysSections.find(item => item.id === 'mcp')!
      ]
    }
  ]

  // 系统：管理员/桌面专属 + 版本信息，收到底部
  const systemItems: SidebarItem[] = []
  if (showAdminDebugSection.value) {
    systemItems.push({
      id: 'debug',
      label: '调试',
      desc: '保存对话请求',
      icon: Bug
    })
  }
  if (isTauriRuntime() && !platformAuth.isStandalone) {
    systemItems.push({ id: 'cloud', label: '云主机', desc: '购买与管理', icon: Cloud })
  }
  systemItems.push(usage)
  systemItems.push(about)
  groups.push({ items: systemItems })

  return groups
})

onMounted(() => {
  initFormFromStore()
})
</script>

<template>
  <div
    class="app-content-no-drag h-full w-full min-h-0 flex flex-col overflow-hidden bg-background"
    data-tauri-drag-region="false"
  >
      <div class="flex h-0 min-h-0 flex-1 overflow-hidden">
        <aside class="flex h-full w-56 shrink-0 flex-col overflow-hidden border-r border-border bg-[hsl(var(--card-elevated))]">
          <WindowDragRegion
            region="settings-top-chrome"
            class="shrink-0 flex items-center select-none pr-2"
            :class="chromeEnabled && macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
          >
            <div
              v-if="chromeEnabled && macTrafficLightPadding"
              class="h-full shrink-0 traffic-light-inset"
              aria-hidden="true"
            />
            <button
              type="button"
              class="min-w-0 flex-1 h-full flex items-center gap-2 text-left transition-colors cursor-pointer group"
              :class="chromeEnabled && macTrafficLightPadding ? 'pl-2' : 'pl-3'"
              title="返回对话"
              aria-label="返回对话"
              @click="emit('close')"
            >
              <ArrowLeft class="w-3.5 h-3.5 shrink-0 text-muted/70 group-hover:text-foreground/80" />
              <span class="truncate text-[13px] font-medium text-foreground/50 group-hover:text-foreground/90">返回对话</span>
            </button>
          </WindowDragRegion>
          <nav class="flex min-h-0 flex-1 flex-col overflow-y-auto pl-3 pr-2 pt-3 pb-3">
            <template v-for="(group, groupIndex) in sections" :key="groupIndex">
              <div v-if="groupIndex > 0" class="my-2 h-px bg-border/60" />
              <button
                v-for="item in group.items"
                :key="item.id"
                class="w-full flex items-center gap-2 rounded-xl py-2.5 text-left transition-all cursor-pointer group"
                :class="activeSection === item.id ? 'bg-hover border border-transparent' : 'border border-transparent hover:bg-hover'"
                @click="activeSection = item.id"
              >
                <component
                  :is="item.icon"
                  class="w-3.5 h-3.5 shrink-0"
                  :class="activeSection === item.id ? 'text-foreground' : 'text-muted'"
                />
                <span class="min-w-0">
                  <span class="block text-[13px] font-medium" :class="activeSection === item.id ? 'text-foreground' : 'text-foreground/80'">{{ item.label }}</span>
                  <span class="block text-[11px] text-muted truncate">{{ item.desc }}</span>
                </span>
              </button>
            </template>
          </nav>
        </aside>

        <div class="flex min-h-0 min-w-0 flex-1 flex-col">
          <WindowDragRegion
            region="main-top-chrome"
            class="shrink-0 flex items-center justify-end select-none"
            :class="chromeEnabled && macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
          >
            <WindowControls
              v-if="useWinLinuxWindowControls"
              class="window-controls-win"
              :maximized="maximized"
              @minimize="minimize"
              @maximize="toggleMaximize"
              @close="closeWindow"
            />
          </WindowDragRegion>
          <main ref="mainEl" class="app-content-no-drag min-h-0 flex-1 overflow-y-auto overscroll-none px-6 pt-3 pb-6" data-tauri-drag-region="false">
            <div
              v-if="renderError"
              class="sticky top-0 z-20 mb-3 px-3 py-2 rounded-lg border border-danger/30 bg-danger/10 text-[11px] text-danger"
            >
              设置页渲染异常：{{ renderError }}
            </div>
            <!-- 保挂载：模型服务编辑在途状态切换分区不丢失（独立于 v-if 链） -->
            <section v-show="activeSection === 'assistant'" class="min-h-full flex flex-col">
              <AssistantSettingsPanel :form="form" />
            </section>

            <section v-if="activeSection === 'channels'" class="min-h-full flex flex-col">
              <ChannelSettingsPanel />
            </section>

            <section v-else-if="activeSection === 'automation'" class="min-h-full flex flex-col">
              <AutomationSettingsPanel @view-session="emit('close')" />
            </section>

            <section v-else-if="activeSection === 'skills'" class="min-h-full flex flex-col">
              <SkillsPanel />
            </section>

            <section v-else-if="activeSection === 'plugins'" class="min-h-full flex flex-col">
              <PluginsPanel />
            </section>

            <section v-else-if="activeSection === 'mcp'" class="min-h-full flex flex-col">
              <McpPanel />
            </section>

            <section v-else-if="activeSection === 'debug'" class="min-h-full flex flex-col">
              <DebugSettingsPanel :form="form" />
            </section>

            <section v-else-if="activeSection === 'models'" class="min-h-full flex flex-col">
              <ModelSettingsPanel :form="form" />
            </section>

            <section v-else-if="activeSection === 'generation'" class="min-h-full flex flex-col">
              <GenerationSettingsPanel :form="form" />
            </section>

            <section v-else-if="activeSection === 'cloud'" class="min-h-full flex flex-col">
              <CloudSettingsPanel :form="form" />
            </section>

            <section v-else-if="activeSection === 'usage'" class="min-h-full flex flex-col">
              <UsageSettingsPanel />
            </section>

            <section v-else-if="activeSection === 'about'" class="min-h-full flex flex-col">
              <AboutSettingsPanel />
            </section>
          </main>
        </div>
      </div>

  </div>
</template>
