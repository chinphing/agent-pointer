<script setup lang="ts">
import { ref, computed } from 'vue'
import { Send, Square, Sparkles, X } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSkillsStore } from '../../stores/skills'
import { useSettingsStore } from '../../stores/settings'

const chat = useChatStore()
const skills = useSkillsStore()
const settings = useSettingsStore()

const text = ref('')
const composing = ref(false)

const canSend = computed(() => text.value.trim().length > 0 && !chat.generating && settings.settings.hasKey)

function send() {
  if (!canSend.value) return
  const v = text.value
  text.value = ''
  chat.sendUserMessage(v)
}

function onKeydown(e: KeyboardEvent) {
  if (composing.value) return
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    send()
  }
}
</script>

<template>
  <div class="px-6 md:px-10 pb-5">
    <div class="max-w-3xl mx-auto">
      <div class="flex items-center justify-between gap-3 mb-2">
        <div class="text-[11px] text-slate-500">
          Agent 模式：<span class="text-primary-cyan">{{ settings.settings.agentMode === 'supervisor' ? 'Supervisor 多 Agent' : 'Single Agent' }}</span>
        </div>
      </div>

      <div v-if="skills.enabledSkills.length" class="flex flex-wrap gap-1.5 mb-2">
        <span v-for="s in skills.enabledSkills" :key="s.id"
              class="inline-flex items-center gap-1 px-2 py-1 rounded-full bg-primary/15 border border-primary/25 text-[11px] text-primary-cyan">
          <Sparkles class="w-3 h-3" />{{ s.name }}
          <button class="ml-0.5 hover:text-white cursor-pointer" @click="skills.toggle(s.id)"><X class="w-3 h-3" /></button>
        </span>
      </div>

      <div class="glass-strong rounded-2xl p-2 flex items-end gap-2 neon-ring">
        <textarea
          v-model="text"
          rows="1"
          class="flex-1 resize-none bg-transparent border-0 outline-none px-3 py-2 text-[15px] text-slate-100 placeholder:text-slate-500 max-h-40"
          :placeholder="settings.settings.hasKey ? '与 ' + settings.settings.model + ' 对话…  Enter 发送，Shift+Enter 换行' : '请先在设置中配置 DashScope API Key'"
          @keydown="onKeydown"
          @compositionstart="composing = true"
          @compositionend="composing = false"
        />
        <button
          v-if="chat.generating"
          class="h-10 w-10 rounded-xl bg-danger/20 hover:bg-danger/30 text-danger flex items-center justify-center cursor-pointer transition"
          @click="chat.stop()"
          title="停止"
        ><Square class="w-4 h-4" /></button>
        <button
          v-else
          class="h-10 w-10 rounded-xl flex items-center justify-center transition"
          :class="canSend
            ? 'bg-gradient-to-r from-primary to-primary-fuchsia text-white shadow-lg shadow-primary/30 hover:opacity-95 cursor-pointer'
            : 'bg-white/5 text-slate-500 cursor-not-allowed'"
          :disabled="!canSend"
          @click="send"
        ><Send class="w-4 h-4" /></button>
      </div>
      <div class="mt-1.5 text-[11px] text-slate-500 text-center">
        Pointer 可能产生不准确信息；工具调用执行前会请求确认。
      </div>
    </div>
  </div>
</template>
