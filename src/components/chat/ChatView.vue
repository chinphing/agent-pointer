<script setup lang="ts">
import { computed } from 'vue'
import MessageList from './MessageList.vue'
import Composer from './Composer.vue'
import { useChatStore } from '../../stores/chat'
import { Sparkles } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'

const chat = useChatStore()
const skills = useSkillsStore()
const empty = computed(() => !chat.current || chat.current.messages.length === 0)
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0">
    <div class="flex-1 overflow-hidden relative">
      <div v-if="empty" class="h-full flex flex-col items-center justify-center px-8 text-center">
        <div class="w-16 h-16 rounded-2xl bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan flex items-center justify-center shadow-xl shadow-primary/30 mb-5">
          <Sparkles class="w-8 h-8 text-white" />
        </div>
        <h1 class="text-3xl font-bold gradient-text mb-2">你好，欢迎来到 Pointer</h1>
        <p class="text-slate-400 max-w-md text-sm leading-6">
          基于阿里云千问的桌面智能助手。你可以让它写代码、解释概念、调用工具或运行已启用的技能。
        </p>
        <div class="mt-6 grid grid-cols-2 gap-3 max-w-xl w-full">
          <div v-for="s in skills.skills.slice(0,4)" :key="s.id"
               class="glass rounded-xl p-3 text-left hover:bg-white/[0.07] transition cursor-pointer"
               @click="skills.toggle(s.id)">
            <div class="flex items-center gap-2">
              <Sparkles class="w-3.5 h-3.5 text-primary-fuchsia" />
              <span class="text-sm font-medium text-slate-100">{{ s.name }}</span>
              <span v-if="skills.isEnabled(s.id)" class="ml-auto text-[10px] text-primary-cyan">已启用</span>
            </div>
            <p class="text-[12px] text-slate-400 mt-1.5 line-clamp-2">{{ s.description }}</p>
          </div>
        </div>
      </div>

      <MessageList v-else />
    </div>
    <Composer />
  </div>
</template>
