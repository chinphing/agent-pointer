<script setup lang="ts">
import { computed, ref } from 'vue'
import { Plus, Search, Settings, MessageSquare, Trash2, Bot, Sparkles, Sun, Moon, Monitor } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { applyTheme } from '../../lib/theme'
import { useWindowChrome } from '../../composables/useWindowChrome'
import WindowControls from './WindowControls.vue'
import type { ThemePreference } from '../../types/chat'

defineEmits<{ (e: 'open-settings'): void; (e: 'open-skills'): void }>()

const chat = useChatStore()
const settings = useSettingsStore()
const {
  enabled: chromeEnabled,
  maximized,
  showCustomControls,
  macTrafficLightPadding,
  minimize,
  toggleMaximize,
  close: closeWindow,
  startDrag
} = useWindowChrome()

function onTitlebarMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  const target = e.target as HTMLElement | null
  if (target?.closest('button, a, input, textarea, select, [data-tauri-drag-region="false"]')) return
  void startDrag()
}

const searchQuery = ref('')

const filteredConversations = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return chat.conversations
  return chat.conversations.filter(c =>
    c.title.toLowerCase().includes(query) ||
    new Date(c.updatedAt).toLocaleString().includes(query)
  )
})

const themeIcon = computed(() => {
  const t = settings.settings.theme ?? 'system'
  if (t === 'light') return Sun
  if (t === 'dark') return Moon
  return Monitor
})

const themeTitle = computed(() => {
  const t = settings.settings.theme ?? 'system'
  if (t === 'light') return '浅色'
  if (t === 'dark') return '深色'
  return '跟随系统'
})

async function cycleTheme() {
  const order: ThemePreference[] = ['system', 'light', 'dark']
  const cur = settings.settings.theme ?? 'system'
  const i = order.indexOf(cur)
  const next = order[(i + 1) % order.length]
  applyTheme(next)
  await settings.save({ theme: next })
}
</script>

<template>
  <div class="h-full w-full flex flex-col">
    <header
      v-if="chromeEnabled"
      class="titlebar hidden md:flex h-10 shrink-0 items-stretch border-b border-border bg-card select-none"
      @mousedown="onTitlebarMouseDown"
    >
      <div
        class="titlebar-brand w-[260px] shrink-0 flex items-center gap-2 px-3"
        :class="macTrafficLightPadding ? 'pl-[4.75rem]' : 'pl-3'"
        data-tauri-drag-region
      >
        <div class="w-7 h-7 rounded-lg bg-accent/15 border border-border flex items-center justify-center shrink-0 pointer-events-none">
          <Bot class="w-3.5 h-3.5 text-accent" />
        </div>
        <div class="text-[13px] font-semibold tracking-wide brand-text truncate pointer-events-none">Pointer</div>
      </div>
      <div class="titlebar-drag flex-1 min-w-0" data-tauri-drag-region />
      <WindowControls
        v-if="showCustomControls"
        :maximized="maximized"
        @minimize="minimize"
        @maximize="toggleMaximize"
        @close="closeWindow"
      />
    </header>

    <div class="flex-1 flex min-h-0">
      <aside class="w-[260px] shrink-0 hidden md:flex flex-col panel border-r">
        <div class="px-3 pb-2 pt-3">
          <div class="flex items-center gap-2">
            <div class="flex-1 flex items-center gap-2 h-9 px-3 rounded-lg panel-elevated">
              <Search class="w-3.5 h-3.5 text-muted" />
              <input
                v-model="searchQuery"
                type="text"
                placeholder="搜索会话"
                class="flex-1 bg-transparent border-0 outline-none text-xs text-foreground placeholder:text-muted"
              />
            </div>
            <button
              class="h-9 w-9 rounded-lg hover:bg-hover flex items-center justify-center cursor-pointer transition"
              @click="chat.newConversation()"
              title="新建会话"
            >
              <Plus class="w-4 h-4 text-muted" />
            </button>
          </div>
        </div>

        <div class="flex-1 overflow-y-auto px-2 pb-3 space-y-1">
          <div
            v-for="c in filteredConversations"
            :key="c.id"
            class="group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition border"
            :class="c.id === chat.currentId
              ? 'bg-accent-muted border-accent/40'
              : 'hover:bg-hover border-transparent'"
            @click="chat.selectConversation(c.id)"
          >
            <MessageSquare class="w-3.5 h-3.5 shrink-0"
              :class="c.id === chat.currentId ? 'text-accent' : 'text-muted'" />
            <div class="flex-1 min-w-0">
              <div class="text-[13px] text-foreground truncate">{{ c.title }}</div>
              <div class="text-[10px] text-muted">{{ new Date(c.updatedAt).toLocaleString() }}</div>
            </div>
            <button
              class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-hover cursor-pointer"
              @click.stop="chat.deleteConversation(c.id)"
              title="删除"
            >
              <Trash2 class="w-3.5 h-3.5 text-muted" />
            </button>
          </div>
          <div v-if="!filteredConversations.length" class="px-3 py-8 text-center text-xs text-muted">
            没有找到匹配的会话
          </div>
        </div>

        <div class="p-3 border-t border-border flex items-center gap-1">
          <button
            class="h-8 w-8 rounded-lg hover:bg-hover flex items-center justify-center cursor-pointer transition"
            @click="cycleTheme"
            :title="`主题：${themeTitle}`"
          >
            <component :is="themeIcon" class="w-4 h-4 text-muted" />
          </button>
          <button
            class="h-8 w-8 rounded-lg hover:bg-hover flex items-center justify-center cursor-pointer transition"
            @click="$emit('open-skills')"
            title="技能库"
          >
            <Sparkles class="w-5 h-5 text-muted" />
          </button>
          <button
            class="h-8 w-8 rounded-lg hover:bg-hover flex items-center justify-center cursor-pointer transition"
            @click="$emit('open-settings')"
            title="设置"
          >
            <Settings class="w-6 h-6 text-muted" />
          </button>
        </div>
      </aside>

      <main class="flex-1 min-w-0 flex flex-col">
        <slot />
      </main>
    </div>
  </div>
</template>
