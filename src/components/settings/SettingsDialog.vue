<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  Bot,
  Check,
  Copy,
  Cpu,
  Database,
  Gauge,
  Plus,
  SlidersHorizontal,
  Trash2,
  Wrench,
  X
} from 'lucide-vue-next'
import type { ProviderConfig } from '../../types/chat'
import { useSettingsStore } from '../../stores/settings'

const emit = defineEmits<{ (e: 'close'): void }>()
const s = useSettingsStore()

const saving = ref(false)
const activeSection = ref('provider')
const copiedKey = ref(false)

const temperature = ref(0.7)
const maxTokens = ref(2048)
const toolApprovalMode = ref<'auto' | 'manual'>('auto')
const agentMode = ref<'single' | 'supervisor'>('single')

const editingProvider = ref<ProviderConfig | null>(null)
const showAddProvider = ref(false)
const editingModelsText = ref('')
const originalApiKey = ref('')
const editingApiKey = ref('')

function maskKey(key: string): string {
  if (!key) return ''
  if (key.length <= 8) return '••••••••'
  return key.slice(0, 4) + '••••••••' + key.slice(-4)
}

const displayKey = computed(() => {
  if (!editingProvider.value) return ''
  if (showAddProvider.value) return ''
  if (editingApiKey.value) return editingApiKey.value
  return maskKey(originalApiKey.value)
})

const inputPlaceholder = computed(() => {
  if (showAddProvider.value) return '请输入 API 密钥'
  return '输入新密钥以替换原密钥'
})

function copyOriginalKey() {
  const key = originalApiKey.value
  if (!key) return
  navigator.clipboard.writeText(key).then(() => {
    copiedKey.value = true
    setTimeout(() => { copiedKey.value = false }, 2000)
  }).catch(e => console.error(e))
}

const sections = [
  { id: 'provider', label: '模型服务', desc: '管理 AI 服务', icon: Cpu },
  { id: 'generation', label: '生成参数', desc: '输出控制', icon: Gauge },
  { id: 'agent', label: '智能模式', desc: '工作方式', icon: Bot },
  { id: 'runtime', label: '运行时', desc: '存储与网络', icon: Database }
]

onMounted(() => {
  temperature.value = s.settings.temperature
  maxTokens.value = s.settings.maxTokens
  toolApprovalMode.value = s.settings.toolApprovalMode || 'auto'
  agentMode.value = s.settings.agentMode || 'single'
})

function startEditProvider(provider: ProviderConfig) {
  editingProvider.value = { ...provider }
  originalApiKey.value = provider.apiKey
  editingApiKey.value = ''
  editingModelsText.value = provider.models.join(', ')
  showAddProvider.value = false
}

function startAddProvider() {
  editingProvider.value = {
    id: '',
    name: '',
    baseUrl: '',
    apiKey: '',
    models: []
  }
  originalApiKey.value = ''
  editingApiKey.value = ''
  editingModelsText.value = ''
  showAddProvider.value = true
}

function cancelEditProvider() {
  editingProvider.value = null
  showAddProvider.value = false
}

function saveProvider() {
  if (!editingProvider.value || !editingProvider.value.id || !editingProvider.value.name || !editingProvider.value.baseUrl) {
    return
  }

  editingProvider.value.models = editingModelsText.value
    .split(',')
    .map(m => m.trim())
    .filter(m => m.length > 0)

  if (editingApiKey.value) {
    editingProvider.value.apiKey = editingApiKey.value
  } else if (!showAddProvider.value) {
    editingProvider.value.apiKey = originalApiKey.value
  }

  if (showAddProvider.value) {
    s.addProvider(editingProvider.value)
  } else {
    s.updateProvider(editingProvider.value.id, editingProvider.value)
  }

  editingProvider.value = null
  showAddProvider.value = false
}

function removeProvider(id: string) {
  s.removeProvider(id)
  if (editingProvider.value?.id === id) {
    editingProvider.value = null
    showAddProvider.value = false
  }
}

async function saveAll() {
  saving.value = true
  try {
    await s.save({
      providers: s.settings.providers,
      activeProviderId: s.settings.activeProviderId,
      model: s.settings.model,
      temperature: Number(temperature.value),
      maxTokens: Number(maxTokens.value),
      toolApprovalMode: toolApprovalMode.value,
      agentMode: agentMode.value
    })
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="emit('close')">
    <div class="w-[920px] max-w-[94vw] h-[720px] max-h-[90vh] glass-strong rounded-2xl border border-white/10 shadow-2xl flex flex-col overflow-hidden">
      <header class="px-5 h-14 flex items-center gap-2 border-b border-white/5 shrink-0">
        <SlidersHorizontal class="w-4 h-4 text-primary-cyan" />
        <div>
          <h2 class="text-base font-semibold text-slate-100">设置</h2>
          <p class="text-[11px] text-slate-500">配置 AI 模型、生成参数和工作模式</p>
        </div>
        <div class="flex-1" />
        <button class="p-2 rounded-lg hover:bg-white/5 cursor-pointer" @click="emit('close')">
          <X class="w-4 h-4 text-slate-300" />
        </button>
      </header>

      <div class="flex flex-1 min-h-0">
        <aside class="w-64 shrink-0 border-r border-white/5 p-3 bg-black/10">
          <button
            v-for="item in sections"
            :key="item.id"
            class="w-full flex items-center gap-3 rounded-xl px-3 py-3 text-left transition cursor-pointer"
            :class="activeSection === item.id ? 'bg-primary/15 border border-primary/30' : 'border border-transparent hover:bg-white/[0.05]'"
            @click="activeSection = item.id"
          >
            <component :is="item.icon" class="w-4 h-4" :class="activeSection === item.id ? 'text-primary-cyan' : 'text-slate-400'" />
            <span class="min-w-0">
              <span class="block text-sm" :class="activeSection === item.id ? 'text-slate-100' : 'text-slate-300'">{{ item.label }}</span>
              <span class="block text-[11px] text-slate-500 truncate">{{ item.desc }}</span>
            </span>
          </button>
        </aside>

        <main class="flex-1 overflow-y-auto p-5">
          <section v-if="activeSection === 'provider'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Cpu class="w-4 h-4 text-primary-cyan" />模型服务管理
              </h3>
              <p class="mt-1 text-xs text-slate-500">添加、编辑或删除 AI 模型服务配置。</p>
            </div>

            <div class="space-y-3">
              <div v-for="p in s.settings.providers" :key="p.id" class="glass rounded-xl p-4 border border-white/5">
                <div class="flex items-start justify-between">
                  <div class="flex-1">
                  <div class="flex items-center gap-2">
                    <span class="text-sm font-medium text-slate-100">{{ p.name }}</span>
                    <span v-if="s.settings.activeProviderId === p.id" class="px-2 py-0.5 rounded-full bg-primary/20 text-[10px] text-primary-cyan">当前使用</span>
                    <span v-if="p.apiKey" class="px-2 py-0.5 rounded-full bg-green-500/20 text-[10px] text-green-400">已配置密钥</span>
                  </div>
                  <p class="mt-1 text-[11px] text-slate-500 truncate">{{ p.baseUrl }}</p>
                  <p class="mt-1 text-[11px] text-slate-500">模型：{{ p.models.join(', ') || '未配置' }}</p>
                </div>
                  <div class="flex items-center gap-2">
                    <button class="p-1.5 rounded-lg hover:bg-white/10 cursor-pointer" @click="startEditProvider(p)">
                      <Wrench class="w-3.5 h-3.5 text-slate-400" />
                    </button>
                    <button class="p-1.5 rounded-lg hover:bg-white/10 cursor-pointer" @click="removeProvider(p.id)">
                      <Trash2 class="w-3.5 h-3.5 text-danger" />
                    </button>
                    <button v-if="s.settings.activeProviderId !== p.id" class="px-2 py-1 rounded-lg bg-primary/20 text-[11px] text-primary-cyan hover:bg-primary/30 cursor-pointer" @click="s.setActiveProvider(p.id)">
                      使用
                    </button>
                  </div>
                </div>
              </div>

              <button class="w-full py-3 rounded-xl border border-dashed border-white/10 text-sm text-slate-400 hover:border-primary/30 hover:text-primary-cyan cursor-pointer flex items-center justify-center gap-2" @click="startAddProvider">
                <Plus class="w-4 h-4" />
                添加模型服务
              </button>
            </div>

            <div v-if="editingProvider" class="glass rounded-xl p-4 border border-primary/30 space-y-4">
              <h4 class="text-sm font-medium text-slate-100">{{ showAddProvider ? '添加模型服务' : '编辑模型服务' }}</h4>
              
              <div class="grid grid-cols-1 gap-3">
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1">服务 ID（唯一标识）</label>
                  <input v-model="editingProvider.id" :disabled="!showAddProvider" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50 disabled:opacity-50" placeholder="例如：qwen, openai" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1">服务名称</label>
                  <input v-model="editingProvider.name" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" placeholder="例如：阿里云千问" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1">API 地址</label>
                  <input v-model="editingProvider.baseUrl" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" placeholder="https://api.example.com/v1" />
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1">API 密钥</label>
                  <div class="flex items-center gap-2 h-10 px-3 rounded-lg glass border border-white/5">
                    <input :value="displayKey" @input="e => { editingApiKey = (e.target as HTMLInputElement).value }" type="password" class="flex-1 bg-transparent border-0 outline-none text-sm text-slate-100 placeholder:text-slate-500" :placeholder="inputPlaceholder" />
                    <button v-if="!showAddProvider && originalApiKey" class="p-1 rounded hover:bg-white/10 cursor-pointer transition" :class="copiedKey ? 'text-green-400' : 'text-slate-400 hover:text-slate-200'" @click="copyOriginalKey" :title="copiedKey ? '已复制' : '复制原始密钥'">
                      <Check v-if="copiedKey" class="w-4 h-4" />
                      <Copy v-else class="w-4 h-4" />
                    </button>
                  </div>
                </div>
                <div>
                  <label class="block text-[12px] text-slate-400 mb-1">模型列表（逗号分隔）</label>
                  <input v-model="editingModelsText" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" placeholder="model-1, model-2, model-3" />
                </div>
              </div>

              <div class="flex items-center gap-2">
                <button class="h-9 px-4 rounded-lg glass hover:bg-white/10 text-sm text-slate-200 cursor-pointer" @click="cancelEditProvider">取消</button>
                <button class="h-9 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50" :disabled="!editingProvider.id || !editingProvider.name || !editingProvider.baseUrl" @click="saveProvider">
                  {{ showAddProvider ? '添加' : '保存' }}
                </button>
              </div>
            </div>
          </section>

          <section v-else-if="activeSection === 'generation'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Gauge class="w-4 h-4 text-primary-cyan" />生成参数
              </h3>
              <p class="mt-1 text-xs text-slate-500">控制 AI 输出的创造性和长度。</p>
            </div>

              <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div class="glass rounded-xl p-4 border border-white/5">
                <label class="block text-[12px] text-slate-400 mb-2">创造性：{{ temperature }}</label>
                <input v-model.number="temperature" type="range" min="0" max="2" step="0.1" class="w-full accent-[#7C3AED]" />
                <p class="mt-2 text-[11px] text-slate-500">越低越稳定，越高越有创造性。</p>
              </div>
              <div class="glass rounded-xl p-4 border border-white/5">
                <label class="block text-[12px] text-slate-400 mb-2">最大输出长度</label>
                <input v-model.number="maxTokens" type="number" min="64" max="32768" step="64" class="w-full h-10 px-3 rounded-lg bg-black/30 border border-white/5 text-sm text-slate-100 outline-none focus:border-primary/50" />
                <p class="mt-2 text-[11px] text-slate-500">限制单次回复的最大长度。</p>
              </div>
            </div>
          </section>

          <section v-else-if="activeSection === 'agent'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Bot class="w-4 h-4 text-primary-cyan" />智能模式
              </h3>
              <p class="mt-1 text-xs text-slate-500">选择 AI 的工作方式和工具使用权限。</p>
            </div>

            <div class="glass rounded-xl p-4 border border-white/5">
              <h4 class="text-sm font-medium text-slate-100 flex items-center gap-2"><Bot class="w-4 h-4 text-primary-fuchsia" />工作模式</h4>
              <div class="mt-3 grid grid-cols-1 md:grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer" :class="agentMode === 'single' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="agentMode" type="radio" value="single" class="sr-only" />
                  <span class="block text-sm text-slate-100">标准模式</span>
                  <span class="mt-1 block text-[11px] text-slate-500">AI 直接处理你的问题，适合大多数场景。</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer" :class="agentMode === 'supervisor' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="agentMode" type="radio" value="supervisor" class="sr-only" />
                  <span class="block text-sm text-slate-100">多专家协作</span>
                  <span class="mt-1 block text-[11px] text-slate-500">AI 自动拆解任务，调度多个专家角色协作完成复杂工作。</span>
                </label>
              </div>
            </div>

            <div class="glass rounded-xl p-4 border border-white/5">
              <h4 class="text-sm font-medium text-slate-100 flex items-center gap-2"><Wrench class="w-4 h-4 text-primary-fuchsia" />工具使用权限</h4>
              <div class="mt-3 grid grid-cols-1 md:grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer" :class="toolApprovalMode === 'auto' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-slate-100">自动执行</span>
                  <span class="mt-1 block text-[11px] text-slate-500">AI 使用工具时自动执行，无需确认。</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer" :class="toolApprovalMode === 'manual' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-slate-100">敏感操作确认</span>
                  <span class="mt-1 block text-[11px] text-slate-500">涉及文件、命令等操作时需要你确认。</span>
                </label>
              </div>
            </div>
          </section>

          <section v-else class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Database class="w-4 h-4 text-primary-cyan" />运行时与存储
              </h3>
              <p class="mt-1 text-xs text-slate-500">查看当前存储与网络运行方式。</p>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div class="glass rounded-xl p-4 border border-white/5">
                <div class="flex items-center gap-2 text-sm text-slate-100"><Database class="w-4 h-4 text-primary-fuchsia" />本地数据</div>
                <p class="mt-2 text-xs text-slate-500">配置、API 密钥和会话记录保存在本机。</p>
              </div>
              <div class="glass rounded-xl p-4 border border-white/5">
                <div class="flex items-center gap-2 text-sm text-slate-100"><Network class="w-4 h-4 text-primary-fuchsia" />网络</div>
                <p class="mt-2 text-xs text-slate-500">当前直接访问 AI 服务 API。</p>
              </div>
            </div>
          </section>
        </main>
      </div>

      <footer class="px-5 h-14 flex items-center gap-3 border-t border-white/5 shrink-0">
        <p class="text-[11px] text-slate-500 flex-1">
          支持标准和多专家协作两种工作模式。
        </p>
        <button class="h-9 px-4 rounded-lg glass hover:bg-white/10 text-sm text-slate-200 cursor-pointer" @click="emit('close')">取消</button>
        <button class="h-9 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50" :disabled="saving" @click="saveAll">
          {{ saving ? '保存中…' : '保存配置' }}
        </button>
      </footer>
    </div>
  </div>
</template>
