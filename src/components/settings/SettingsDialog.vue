<script setup lang="ts">
import { computed, nextTick, onErrorCaptured, onMounted, ref, watch } from 'vue'
import { ArrowLeft, Bug, Sun, Moon, Monitor, Sparkles, Bot, Cpu, Gauge, MessageSquare, UserCircle, Cloud, Clock, Info, Settings } from 'lucide-vue-next'
import { isTauriRuntime } from '../../lib/runtime'
import { useWindowChrome } from '../../composables/useWindowChrome'
import WindowDragRegion from '../layout/WindowDragRegion.vue'
import { useSettingsStore } from '../../stores/settings'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { provideSettingsDialogForm } from '../../composables/useSettingsDialogForm'
import ChannelSettingsPanel from './ChannelSettingsPanel.vue'
import AssistantSettingsPanel from './panels/AssistantSettingsPanel.vue'
import GenerationSettingsPanel from './panels/GenerationSettingsPanel.vue'
import ModelSettingsPanel from './panels/ModelSettingsPanel.vue'
import DebugSettingsPanel from './panels/DebugSettingsPanel.vue'
import AccountSettingsPanel from './panels/AccountSettingsPanel.vue'
import CloudSettingsPanel from './panels/CloudSettingsPanel.vue'
import AutomationSettingsPanel from './panels/AutomationSettingsPanel.vue'
import AboutSettingsPanel from './panels/AboutSettingsPanel.vue'
import SkillsPanel from '../skills/SkillsPanel.vue'

const emit = defineEmits<{
  (e: 'close'): void
}>()
const props = withDefaults(defineProps<{
  initialSection?: string
}>(), {
  initialSection: 'account'
})

const s = useSettingsStore()
const platformAuth = usePlatformAuthStore()

const activeSection = ref(props.initialSection)
const mainEl = ref<HTMLElement | null>(null)
const renderError = ref('')
const { enabled: chromeEnabled, macTrafficLightPadding } = useWindowChrome()

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
  theme,
  themeLabel,
  cycleTheme,
  currentThemeIcon,
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
  const account: SidebarItem = {
    id: 'account',
    label: '账户',
    desc: '余额、登录与凭据',
    icon: UserCircle
  }
  const about: SidebarItem = {
    id: 'about',
    label: '关于',
    desc: '版本与更新',
    icon: Info
  }

  const groups: SidebarGroup[] = [
    // 账户：高频，单独置顶
    { items: [account] },
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
        alwaysSections.find(item => item.id === 'skills')!
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
    class="app-content-no-drag h-full w-full min-h-0 flex flex-col bg-background"
    data-tauri-drag-region="false"
  >
      <!-- Header: reserve the native macOS traffic-light zone and drag from empty space. -->
      <WindowDragRegion
        as="header"
        region="settings-top-chrome"
        class="px-6 h-14 flex items-center gap-2 border-b border-border shrink-0"
        :class="chromeEnabled && macTrafficLightPadding ? 'pl-[4.75rem]' : ''"
      >
        <div class="flex-1" />
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
      </WindowDragRegion>

      <div class="flex flex-1 min-h-0">
        <!-- Sidebar -->
        <aside class="w-56 shrink-0 border-r border-border p-3 bg-[hsl(var(--card-elevated))]">
          <!-- 返回按钮：仿栏位结构但弱化（muted 色、hover 才加深，不参与选中态） -->
          <button
            type="button"
            class="w-full flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-all cursor-pointer group"
            title="返回对话"
            aria-label="返回对话"
            @click="emit('close')"
          >
            <div class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors bg-hover/40 group-hover:bg-hover/80">
              <ArrowLeft class="w-3.5 h-3.5 text-muted/70 group-hover:text-foreground/80" />
            </div>
            <span class="block min-w-0 text-[13px] font-medium text-foreground/50 group-hover:text-foreground/90">返回对话</span>
          </button>
          <div class="my-2 h-px bg-border/60" />
          <template v-for="(group, groupIndex) in sections" :key="groupIndex">
            <div v-if="groupIndex > 0" class="my-2 h-px bg-border/60" />
            <button
              v-for="item in group.items"
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
          </template>
        </aside>

        <!-- Main Content -->
        <main ref="mainEl" class="app-content-no-drag flex-1 overflow-y-auto" data-tauri-drag-region="false">
          <div
            v-if="renderError"
            class="sticky top-0 z-20 mx-4 mt-3 px-3 py-2 rounded-lg border border-red-400/30 bg-red-400/10 text-[11px] text-red-400"
          >
            设置页渲染异常：{{ renderError }}
          </div>
          <!-- ==================== Assistant Section ==================== -->
          <!-- 保挂载：模型服务编辑在途状态切换分区不丢失（独立于 v-if 链） -->
          <section v-show="activeSection === 'assistant'" class="p-6 min-h-full flex flex-col">
            <AssistantSettingsPanel :form="form" />
          </section>

          <section v-if="activeSection === 'channels'" class="p-6 min-h-full flex flex-col">
            <ChannelSettingsPanel />
          </section>

          <section v-else-if="activeSection === 'automation'" class="p-6 min-h-full flex flex-col">
            <AutomationSettingsPanel @view-session="emit('close')" />
          </section>

          <section v-else-if="activeSection === 'skills'" class="p-6 min-h-full flex flex-col">
            <SkillsPanel />
          </section>

          <section v-else-if="activeSection === 'debug'" class="p-6 min-h-full flex flex-col">
            <DebugSettingsPanel :form="form" />
          </section>

          <!-- ==================== Model Section ==================== -->
          <section v-else-if="activeSection === 'models'" class="p-6 min-h-full flex flex-col">
            <ModelSettingsPanel :form="form" />
          </section>

          <!-- ==================== Generation Section (系统设置) ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 min-h-full flex flex-col">
            <GenerationSettingsPanel :form="form" />
          </section>

          <!-- ==================== Platform account (desktop) ==================== -->
          <section v-else-if="activeSection === 'account'" class="p-6 min-h-full flex flex-col">
            <AccountSettingsPanel :form="form" />
          </section>

          <section v-else-if="activeSection === 'cloud'" class="p-6 min-h-full flex flex-col">
            <CloudSettingsPanel :form="form" />
          </section>

          <!-- About Settings -->
          <section v-else-if="activeSection === 'about'" class="p-6 min-h-full flex flex-col">
            <AboutSettingsPanel />
          </section>
        </main>
      </div>

  </div>
</template>
