<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ArrowLeft, Bug, Sun, Moon, Monitor, Sparkles, Bot, Cpu, Gauge, MessageSquare, UserCircle, Cloud, Clock, Info } from 'lucide-vue-next'
import { isTauriRuntime } from '../../lib/runtime'
import { useWindowChrome } from '../../composables/useWindowChrome'
import WindowDragRegion from '../layout/WindowDragRegion.vue'
import { useSettingsStore } from '../../stores/settings'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { provideSettingsDialogForm } from '../../composables/useSettingsDialogForm'
import ProviderSettingsPanel from './ProviderSettingsPanel.vue'
import ChannelSettingsPanel from './ChannelSettingsPanel.vue'
import AssistantSettingsPanel from './panels/AssistantSettingsPanel.vue'
import GenerationSettingsPanel from './panels/GenerationSettingsPanel.vue'
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
const { enabled: chromeEnabled, macTrafficLightPadding } = useWindowChrome()

const alwaysSections = [
  { id: 'automation', label: '自动化', desc: '定时任务与 Webhook', icon: Clock },
  { id: 'channels', label: '连接', desc: '微信/飞书/企微/钉钉', icon: MessageSquare },
  { id: 'skills', label: '技能', desc: '启用与管理技能', icon: Sparkles },
  { id: 'assistant', label: '智能体', desc: 'Computer 与工具权限', icon: Bot },
  { id: 'generation', label: '界面配置', desc: '界面显示选项', icon: Gauge },
  { id: 'provider', label: '模型服务', desc: '平台/自定义模型服务', icon: Cpu }
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
  label?: string
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
    // 账户：高频，单独置顶、无分组标题
    { items: [account] },
    // 智能体与模型：决定 AI 怎么工作、怎么显示、用哪些模型
    {
      label: '智能体与模型',
      items: [
        alwaysSections.find(item => item.id === 'assistant')!,
        alwaysSections.find(item => item.id === 'generation')!,
        alwaysSections.find(item => item.id === 'provider')!
      ]
    },
    // 自动化与集成：外部接入
    {
      label: '自动化与集成',
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
    systemItems.push(debugSections[0] as unknown as SidebarItem)
  }
  if (isTauriRuntime() && !platformAuth.isStandalone) {
    systemItems.push({ id: 'cloud', label: '云主机', desc: '购买与管理', icon: Cloud })
  }
  systemItems.push(about)
  groups.push({ label: '系统', items: systemItems })

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
        <button
          type="button"
          class="h-8 px-2 rounded-md border border-border text-foreground hover:bg-hover transition-colors inline-flex items-center gap-1 cursor-pointer"
          title="返回聊天"
          aria-label="返回聊天"
          @click="emit('close')"
        >
          <ArrowLeft class="w-4 h-4" />
          <span class="text-xs">返回</span>
        </button>
      </WindowDragRegion>

      <div class="flex flex-1 min-h-0">
        <!-- Sidebar -->
        <aside class="w-56 shrink-0 border-r border-border p-3 bg-[hsl(var(--card-elevated))]">
          <template v-for="group in sections" :key="group.label ?? 'account'">
            <p
              v-if="group.label"
              class="px-3 pt-4 pb-1 text-[10px] font-medium text-muted/70 uppercase tracking-wider"
            >{{ group.label }}</p>
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
        <main class="app-content-no-drag flex-1 overflow-y-auto" data-tauri-drag-region="false">
          <!-- ==================== Assistant Section ==================== -->
          <section v-if="activeSection === 'assistant'" class="p-6 space-y-5">
            <AssistantSettingsPanel :form="form" />
          </section>

          <section v-else-if="activeSection === 'channels'" class="p-6">
            <ChannelSettingsPanel />
          </section>

          <section v-else-if="activeSection === 'automation'" class="p-6 space-y-5">
            <AutomationSettingsPanel @view-session="emit('close')" />
          </section>

          <section v-else-if="activeSection === 'skills'" class="p-6">
            <SkillsPanel />
          </section>

          <section v-else-if="activeSection === 'debug'" class="p-6">
            <DebugSettingsPanel :form="form" />
          </section>

          <!-- ==================== Generation Section ==================== -->
          <section v-else-if="activeSection === 'generation'" class="p-6 space-y-5">
            <GenerationSettingsPanel :form="form" />
          </section>

          <!-- ==================== Platform account (desktop) ==================== -->
          <section v-else-if="activeSection === 'account'" class="p-6 space-y-5">
            <AccountSettingsPanel :form="form" />
          </section>

          <section v-else-if="activeSection === 'cloud'" class="p-6 space-y-5">
            <CloudSettingsPanel :form="form" />
          </section>

          <!-- About Settings -->
          <section v-else-if="activeSection === 'about'" class="p-6 space-y-5">
            <AboutSettingsPanel />
          </section>

          <!-- Keep the provider panel mounted to preserve in-progress edits while switching sections. -->
          <section v-show="activeSection === 'provider'" class="p-6">
            <ProviderSettingsPanel :form="form" />
          </section>
        </main>
      </div>

  </div>
</template>
