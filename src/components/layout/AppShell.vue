<script setup lang="ts">
import { computed } from 'vue'
import { Plus, Search, Settings, Sparkles, MessageSquare, Trash2, Bot, Cpu } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { useSkillsStore } from '../../stores/skills'

defineEmits<{ (e: 'open-settings'): void; (e: 'open-skills'): void }>()

const chat = useChatStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const enabledCount = computed(() => skills.enabledIds.length)
</script>

<template>
  <div class="h-full w-full flex flex-col">
    <!-- Top bar -->
    <header class="fixed top-0 inset-x-0 h-14 z-30 glass-strong flex items-center px-5 gap-4">
      <div class="flex items-center gap-2">
        <div class="w-8 h-8 rounded-lg bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan flex items-center justify-center shadow-lg shadow-primary/30">
          <Bot class="w-4 h-4 text-white" />
        </div>
        <div class="leading-tight">
          <div class="text-[15px] font-semibold tracking-wide gradient-text">Pointer</div>
          <div class="text-[11px] text-slate-400 -mt-0.5">AI 工作台 · Tauri × Vue</div>
        </div>
      </div>

      <div class="flex-1" />

      <div class="hidden md:flex items-center gap-2 px-3 h-9 rounded-full glass text-xs text-slate-300">
        <Cpu class="w-3.5 h-3.5 text-primary-cyan" />
        <span class="text-slate-400">模型</span>
        <span class="font-medium text-slate-100">{{ settings.settings.model }}</span>
        <span class="mx-1 w-1 h-1 rounded-full bg-white/20" />
        <span :class="settings.settings.hasKey ? 'text-success' : 'text-warning'">
          {{ settings.settings.hasKey ? '已连接' : '未配置 Key' }}
        </span>
      </div>

      <button
        class="h-9 px-3 rounded-full glass hover:bg-white/10 flex items-center gap-1.5 text-xs text-slate-200 cursor-pointer transition"
        @click="$emit('open-skills')"
      >
        <Sparkles class="w-3.5 h-3.5 text-primary-fuchsia" />
        Skills
        <span v-if="enabledCount" class="ml-0.5 px-1.5 py-0.5 rounded-full bg-primary/30 text-primary-cyan text-[10px]">
          {{ enabledCount }}
        </span>
      </button>

      <button
        class="h-9 w-9 rounded-full glass hover:bg-white/10 flex items-center justify-center cursor-pointer transition"
        @click="$emit('open-settings')"
        title="设置"
      >
        <Settings class="w-4 h-4 text-slate-200" />
      </button>
    </header>

    <!-- Main -->
    <div class="flex-1 flex pt-14 min-h-0">
      <!-- Sidebar -->
      <aside class="w-[260px] shrink-0 hidden md:flex flex-col glass border-r border-white/5">
        <div class="p-3 flex items-center gap-2">
          <button
            class="flex-1 h-10 rounded-xl bg-gradient-to-r from-primary to-primary-fuchsia hover:opacity-95 flex items-center justify-center gap-2 text-sm font-medium text-white shadow-lg shadow-primary/20 cursor-pointer transition"
            @click="chat.newConversation()"
          >
            <Plus class="w-4 h-4" />新建会话
          </button>
        </div>

        <div class="px-3 pb-2">
          <div class="flex items-center gap-2 h-9 px-3 rounded-lg glass">
            <Search class="w-3.5 h-3.5 text-slate-400" />
            <input
              type="text"
              placeholder="搜索会话"
              class="flex-1 bg-transparent border-0 outline-none text-xs text-slate-200 placeholder:text-slate-500"
            />
          </div>
        </div>

        <div class="flex-1 overflow-y-auto px-2 pb-3 space-y-1">
          <div
            v-for="c in chat.conversations"
            :key="c.id"
            class="group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition"
            :class="c.id === chat.currentId
              ? 'bg-primary/15 border border-primary/30'
              : 'hover:bg-white/5 border border-transparent'"
            @click="chat.selectConversation(c.id)"
          >
            <MessageSquare class="w-3.5 h-3.5 shrink-0"
              :class="c.id === chat.currentId ? 'text-primary-cyan' : 'text-slate-400'" />
            <div class="flex-1 min-w-0">
              <div class="text-[13px] text-slate-100 truncate">{{ c.title }}</div>
              <div class="text-[10px] text-slate-500">{{ new Date(c.updatedAt).toLocaleString() }}</div>
            </div>
            <button
              class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-white/10 cursor-pointer"
              @click.stop="chat.deleteConversation(c.id)"
              title="删除"
            >
              <Trash2 class="w-3.5 h-3.5 text-slate-400" />
            </button>
          </div>
        </div>
      </aside>

      <!-- Content -->
      <main class="flex-1 min-w-0 flex flex-col">
        <slot />
      </main>
    </div>
  </div>
</template>
