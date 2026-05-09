<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { Bot, ChevronDown, FolderOpen, Send, Sparkles, Square, Users } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import type { AgentDef, AgentProfile } from '../../types/chat'
import { listAgents } from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'

const chat = useChatStore()
const settings = useSettingsStore()

const text = ref('')
const composing = ref(false)
const showModelPicker = ref(false)
const showAgentPicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const modelBtnRef = ref<HTMLButtonElement | null>(null)
const agentBtnRef = ref<HTMLButtonElement | null>(null)
const modelPickerRef = ref<HTMLDivElement | null>(null)
const agentPickerRef = ref<HTMLDivElement | null>(null)
const modelPickerWidth = ref(0)
const agentPickerWidth = ref(0)

const agents = ref<AgentDef[]>([])

const workers = computed(() => agents.value.filter(a => a.role === 'worker' && a.enabled))
const supervisorAgent = computed(
  () =>
    agents.value.find(a => a.id === 'supervisor' && a.enabled) ||
    agents.value.find(a => a.role === 'supervisor')
)

function isCoderProfile(p: AgentProfile): boolean {
  return p === 'coder'
}

function agentNeedsWorkspace(a: AgentDef | undefined): boolean {
  if (!a) return false
  return a.id === 'coder' || isCoderProfile(a.profile)
}

const selectedWorker = computed(() => {
  if (settings.settings.agentMode !== 'single') return undefined
  const id = settings.settings.leadAgentId?.trim()
  if (!id) return workers.value.find(w => w.id === 'default')
  return workers.value.find(w => w.id === id)
})

const needsWorkspace = computed(() => agentNeedsWorkspace(selectedWorker.value))

const currentAgentLabel = computed(() => {
  if (settings.settings.agentMode === 'supervisor') {
    return supervisorAgent.value?.name ?? 'Supervisor'
  }
  return selectedWorker.value?.name ?? 'Default Agent'
})

const canSend = computed(
  () =>
    text.value.trim().length > 0 &&
    !chat.generating &&
    settings.settings.hasKey &&
    (!needsWorkspace.value || !!settings.settings.workspaceRoot?.trim())
)

async function loadAgentsList() {
  try {
    agents.value = await listAgents()
  } catch (e) {
    console.error(e)
  }
}

async function pickWorkspaceFolder() {
  if (!isTauriRuntime()) return
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const dir = await open({ directory: true, multiple: false })
    if (typeof dir === 'string' && dir) {
      await settings.save({ workspaceRoot: dir })
    }
  } catch (e) {
    console.error(e)
  }
}

function send() {
  if (!canSend.value) return
  if (needsWorkspace.value && !settings.settings.workspaceRoot?.trim()) return
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

async function selectSupervisorMode() {
  await settings.save({ agentMode: 'supervisor', leadAgentId: '' })
  showAgentPicker.value = false
}

async function selectWorkerAgent(agent: AgentDef) {
  await settings.save({ agentMode: 'single', leadAgentId: agent.id })
  showAgentPicker.value = false
}

function autoResize() {
  if (!textareaRef.value) return
  textareaRef.value.style.height = 'auto'
  textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 250) + 'px'
}

function updatePickerWidths() {
  if (modelBtnRef.value) {
    const btnWidth = modelBtnRef.value.offsetWidth
    modelPickerWidth.value = Math.max(btnWidth, 180)
  }
  if (agentBtnRef.value) {
    const btnWidth = agentBtnRef.value.offsetWidth
    agentPickerWidth.value = Math.max(btnWidth, 200)
  }
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (showModelPicker.value && modelBtnRef.value && modelPickerRef.value) {
    if (!modelBtnRef.value.contains(target) && !modelPickerRef.value.contains(target)) {
      showModelPicker.value = false
    }
  }
  if (showAgentPicker.value && agentBtnRef.value && agentPickerRef.value) {
    if (!agentBtnRef.value.contains(target) && !agentPickerRef.value.contains(target)) {
      showAgentPicker.value = false
    }
  }
}

watch(
  () => [showModelPicker.value, showAgentPicker.value],
  () => nextTick(() => updatePickerWidths())
)

onMounted(() => {
  loadAgentsList()
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
      <div v-if="needsWorkspace" class="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-slate-400">
        <span class="text-amber-200/90">编码智能体需要工作区目录</span>
        <span class="truncate max-w-[min(100%,280px)] text-slate-500" :title="settings.settings.workspaceRoot">
          {{ settings.settings.workspaceRoot || '未选择' }}
        </span>
        <button
          v-if="isTauriRuntime()"
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-lg bg-white/10 hover:bg-white/15 text-slate-200 cursor-pointer"
          @click="pickWorkspaceFolder"
        >
          <FolderOpen class="w-3 h-3" />
          选择文件夹
        </button>
        <input
          v-else
          v-model="settings.settings.workspaceRoot"
          type="text"
          placeholder="绝对路径（Web）"
          class="flex-1 min-w-[120px] h-8 px-2 rounded-lg bg-black/30 border border-white/10 text-slate-200 text-xs"
          @change="settings.save({ workspaceRoot: settings.settings.workspaceRoot })"
        />
      </div>

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
            ref="agentBtnRef"
            class="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg glass text-[11px] text-slate-300 hover:bg-white/10 cursor-pointer max-w-[220px]"
            @click="showAgentPicker = !showAgentPicker"
          >
            <Bot v-if="settings.settings.agentMode === 'single'" class="w-3 h-3 shrink-0 text-primary-fuchsia" />
            <Users v-else class="w-3 h-3 shrink-0 text-primary-fuchsia" />
            <span class="truncate">{{ currentAgentLabel }}</span>
            <ChevronDown class="w-3 h-3 shrink-0" />
          </button>

          <div v-if="showAgentPicker" ref="agentPickerRef" class="absolute bottom-full left-0 mb-2 glass-strong rounded-xl shadow-2xl overflow-hidden z-50 max-h-72 overflow-y-auto" :style="{ minWidth: agentPickerWidth + 'px' }">
            <div class="p-2 border-b border-white/5">
              <div class="text-[11px] text-slate-500">执行智能体（名称来自配置）</div>
            </div>
            <div class="p-1.5 space-y-0.5">
              <button
                v-for="w in workers"
                :key="w.id"
                class="w-full text-left px-3 py-2 rounded-lg text-sm hover:bg-white/5 cursor-pointer flex items-center gap-2"
                :class="settings.settings.agentMode === 'single' && (settings.settings.leadAgentId === w.id || ((!settings.settings.leadAgentId || settings.settings.leadAgentId === 'default') && w.id === 'default')) ? 'bg-primary/15 text-primary-cyan' : 'text-slate-300'"
                @click="selectWorkerAgent(w)"
              >
                <Bot class="w-3 h-3 shrink-0" />
                <span class="min-w-0 truncate">{{ w.name }}</span>
                <span class="text-[10px] text-slate-500 shrink-0">{{ w.id }}</span>
              </button>
              <button
                v-if="supervisorAgent"
                class="w-full text-left px-3 py-2 rounded-lg text-sm hover:bg-white/5 cursor-pointer flex items-center gap-2"
                :class="settings.settings.agentMode === 'supervisor' ? 'bg-primary/15 text-primary-cyan' : 'text-slate-300'"
                @click="selectSupervisorMode"
              >
                <Users class="w-3 h-3 shrink-0" />
                <span class="truncate">{{ supervisorAgent.name }}</span>
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
