<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { Bot, ChevronDown, Send, Square, Sparkles, Users } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'

const chat = useChatStore()
const settings = useSettingsStore()

const text = ref('')
const composing = ref(false)
const showModelPicker = ref(false)
const showModePicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const modelBtnRef = ref<HTMLButtonElement | null>(null)
const modeBtnRef = ref<HTMLButtonElement | null>(null)
const modelPickerRef = ref<HTMLDivElement | null>(null)
const modePickerRef = ref<HTMLDivElement | null>(null)
const modelPickerWidth = ref(0)
const modePickerWidth = ref(0)

const agentModes = [
  { value: 'single', label: '标准模式', icon: Bot },
  { value: 'supervisor', label: '多专家协作', icon: Users },
]

const canSend = computed(() => text.value.trim().length > 0 && !chat.generating && settings.settings.hasKey)

function send() {
  if (!canSend.value) return
  const v = text.value
  text.value = ''
  chat.sendUserMessage(v)
  nextTick(() => {
    if (textareaRef.value) {
      textareaRef.value.style.height = 'auto'
    }
  })
}

function onKeydown(e: KeyboardEvent) {
  if (composing.value) return
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    send()
  }
}

function selectModel(model: string) {
  settings.save({ model })
  showModelPicker.value = false
}

function selectAgentMode(mode: string) {
  settings.save({ agentMode: mode })
  showModePicker.value = false
}

function autoResize() {
  if (!textareaRef.value) return
  textareaRef.value.style.height = 'auto'
  textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 250) + 'px'
}

function updatePickerWidths() {
  if (modelBtnRef.value) {
    const btnWidth = modelBtnRef.value.offsetWidth
    const minWidth = Math.max(btnWidth, 180)
    modelPickerWidth.value = minWidth
  }
  if (modeBtnRef.value) {
    const btnWidth = modeBtnRef.value.offsetWidth
    const minWidth = Math.max(btnWidth, 160)
    modePickerWidth.value = minWidth
  }
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (showModelPicker.value && modelBtnRef.value && modelPickerRef.value) {
    if (!modelBtnRef.value.contains(target) && !modelPickerRef.value.contains(target)) {
      showModelPicker.value = false
    }
  }
  if (showModePicker.value && modeBtnRef.value && modePickerRef.value) {
    if (!modeBtnRef.value.contains(target) && !modePickerRef.value.contains(target)) {
      showModePicker.value = false
    }
  }
}

onMounted(() => {
  updatePickerWidths()
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <div class="px-6 md:px-10 pb-5">
    <div class="max-w-3xl mx-auto">
      <div class="glass-strong rounded-2xl p-2 neon-ring">
        <div class="flex items-end gap-2">
          <textarea
            ref="textareaRef"
            v-model="text"
            rows="1"
            class="flex-1 resize-none bg-transparent border-0 outline-none px-3 py-2 text-[15px] text-slate-100 placeholder:text-slate-500"
            style="max-height: 250px; min-height: 24px;"
            :placeholder="settings.settings.hasKey ? '与 Pointer 对话…' : '请先在设置中配置 API Key'"
            @keydown="onKeydown"
            @input="autoResize"
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
      </div>

      <div class="flex items-center gap-2 mt-2">
        <div class="relative">
          <button
            ref="modelBtnRef"
            class="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg glass text-[11px] text-slate-300 hover:bg-white/10 cursor-pointer"
            @click="showModelPicker = !showModelPicker"
          >
            <Sparkles class="w-3 h-3 text-primary-cyan" />
            {{ settings.activeProvider.name }} / {{ settings.settings.model }}
            <ChevronDown class="w-3 h-3" />
          </button>

          <div v-if="showModelPicker" ref="modelPickerRef" class="absolute bottom-full left-0 mb-2 glass-strong rounded-xl shadow-2xl overflow-hidden z-50" :style="{ minWidth: modelPickerWidth + 'px' }">
            <div class="p-2 border-b border-white/5">
              <div class="text-[11px] text-slate-500">选择模型</div>
            </div>
            <div class="max-h-60 overflow-y-auto p-1.5 space-y-0.5">
              <button
                v-for="m in settings.activeModelList"
                :key="m"
                class="w-full text-left px-3 py-2 rounded-lg text-sm hover:bg-white/5 cursor-pointer whitespace-nowrap"
                :class="settings.settings.model === m ? 'bg-primary/15 text-primary-cyan' : 'text-slate-300'"
                @click="selectModel(m)"
              >
                {{ m }}
              </button>
            </div>
          </div>
        </div>

        <div class="relative">
          <button
            ref="modeBtnRef"
            class="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg glass text-[11px] text-slate-300 hover:bg-white/10 cursor-pointer"
            @click="showModePicker = !showModePicker"
          >
            <Bot v-if="settings.settings.agentMode === 'single'" class="w-3 h-3 text-primary-fuchsia" />
            <Users v-else class="w-3 h-3 text-primary-fuchsia" />
            {{ settings.settings.agentMode === 'single' ? '标准模式' : '多专家协作' }}
            <ChevronDown class="w-3 h-3" />
          </button>

          <div v-if="showModePicker" ref="modePickerRef" class="absolute bottom-full left-0 mb-2 glass-strong rounded-xl shadow-2xl overflow-hidden z-50" :style="{ minWidth: modePickerWidth + 'px' }">
            <div class="p-2 border-b border-white/5">
              <div class="text-[11px] text-slate-500">选择模式</div>
            </div>
            <div class="p-1.5 space-y-0.5">
              <button
                v-for="mode in agentModes"
                :key="mode.value"
                class="w-full text-left px-3 py-2 rounded-lg text-sm hover:bg-white/5 cursor-pointer flex items-center gap-2 whitespace-nowrap"
                :class="settings.settings.agentMode === mode.value ? 'bg-primary/15 text-primary-cyan' : 'text-slate-300'"
                @click="selectAgentMode(mode.value)"
              >
                <component :is="mode.icon" class="w-3 h-3" />
                {{ mode.label }}
              </button>
            </div>
          </div>
        </div>

        <div class="flex-1" />

        <span class="text-[10px] text-slate-600">
          {{ settings.settings.hasKey ? '已连接' : '未配置 Key' }}
        </span>
      </div>
    </div>
  </div>
</template>
