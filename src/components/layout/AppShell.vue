<script setup lang="ts">
import { computed, ref } from 'vue'
import { Plus, Search, Settings, MessageSquare, Trash2, Bot, Sparkles } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'

defineEmits<{ (e: 'open-settings'): void; (e: 'open-skills'): void }>()

const chat = useChatStore()
const settings = useSettingsStore()

const searchQuery = ref('')

const filteredConversations = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return chat.conversations
  return chat.conversations.filter(c =>
    c.title.toLowerCase().includes(query) ||
    new Date(c.updatedAt).toLocaleString().includes(query)
  )
})
</script>

<template>
  <div class="h-full w-full flex flex-col">
    <!-- Main -->
    <div class="flex-1 flex min-h-0">
      <!-- Sidebar -->
      <aside class="w-[260px] shrink-0 hidden md:flex flex-col glass border-r border-white/5">
        <div class="p-3 flex items-center gap-2">
          <div class="flex items-center gap-2 px-2 py-1">
            <div class="w-7 h-7 rounded-lg bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan flex items-center justify-center shadow-lg shadow-primary/30">
              <Bot class="w-3.5 h-3.5 text-white" />
            </div>
            <div class="leading-tight">
              <div class="text-[13px] font-semibold tracking-wide gradient-text">Pointer</div>
            </div>
          </div>
        </div>

        <div class="px-3 pb-2">
          <div class="flex items-center gap-2">
            <div class="flex-1 flex items-center gap-2 h-9 px-3 rounded-lg glass">
              <Search class="w-3.5 h-3.5 text-slate-400" />
              <input
                v-model="searchQuery"
                type="text"
                placeholder="搜索会话"
                class="flex-1 bg-transparent border-0 outline-none text-xs text-slate-200 placeholder:text-slate-500"
              />
            </div>
            <button
              class="h-9 w-9 rounded-lg hover:bg-white/5 flex items-center justify-center cursor-pointer transition"
              @click="chat.newConversation()"
              title="新建会话"
            >
              <Plus class="w-4 h-4 text-slate-400" />
            </button>
          </div>
        </div>

        <div class="flex-1 overflow-y-auto px-2 pb-3 space-y-1">
          <div
            v-for="c in filteredConversations"
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
          <div v-if="!filteredConversations.length" class="px-3 py-8 text-center text-xs text-slate-500">
            没有找到匹配的会话
          </div>
        </div>

        <div class="p-3 border-t border-white/5 flex items-center gap-1">
          <button
            class="h-8 w-8 rounded-lg hover:bg-white/5 flex items-center justify-center cursor-pointer transition"
            @click="$emit('open-skills')"
            title="技能库"
          >
            <Sparkles class="w-5 h-5 text-slate-400" />
          </button>
          <button
            class="h-8 w-8 rounded-lg hover:bg-white/5 flex items-center justify-center cursor-pointer transition"
            @click="$emit('open-settings')"
            title="设置"
          >
            <Settings class="w-6 h-6 text-slate-400" />
          </button>
        </div>
      </aside>

      <!-- Content -->
      <main class="flex-1 min-w-0 flex flex-col">
        <slot />
      </main>
    </div>
  </div>
</template>
