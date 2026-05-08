<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  Bot,
  CheckCircle2,
  Cpu,
  Database,
  Eye,
  EyeOff,
  Gauge,
  KeyRound,
  Loader2,
  Network,
  Shield,
  SlidersHorizontal,
  Wrench,
  X,
  XCircle,
  Zap
} from 'lucide-vue-next'
import { useSettingsStore } from '../../stores/settings'

const emit = defineEmits<{ (e: 'close'): void }>()
const s = useSettingsStore()

const localKey = ref('')
const showKey = ref(false)
const saving = ref(false)
const activeSection = ref('provider')

const provider = ref('qwen')
const baseUrl = ref('')
const model = ref('')
const temperature = ref(0.7)
const maxTokens = ref(2048)
const toolApprovalMode = ref<'auto' | 'manual'>('auto')

const sections = [
  { id: 'provider', label: '模型服务', desc: 'Provider / Endpoint', icon: Cpu },
  { id: 'credential', label: '凭据', desc: 'API Key / Secret', icon: KeyRound },
  { id: 'generation', label: '生成参数', desc: 'Sampling / Tokens', icon: Gauge },
  { id: 'agent', label: 'Agent 能力', desc: 'Tools / Skills / Approval', icon: Bot },
  { id: 'runtime', label: '运行时', desc: 'Storage / Network', icon: Database }
]

const providers = [
  {
    id: 'qwen',
    name: '阿里云千问 DashScope',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    models: ['qwen-plus', 'qwen-turbo', 'qwen-max', 'qwen2.5-coder-32b-instruct']
  },
  {
    id: 'openai',
    name: 'OpenAI Compatible',
    baseUrl: 'https://api.openai.com/v1',
    models: ['gpt-4o-mini', 'gpt-4o']
  },
  {
    id: 'local',
    name: '本地 / 私有兼容服务',
    baseUrl: 'http://127.0.0.1:11434/v1',
    models: ['qwen2.5', 'llama3.1']
  }
]

const currentProvider = computed(() => providers.find(p => p.id === provider.value) || providers[0])

onMounted(() => {
  provider.value = s.settings.provider || 'qwen'
  baseUrl.value = s.settings.baseUrl
  model.value = s.settings.model
  temperature.value = s.settings.temperature
  maxTokens.value = s.settings.maxTokens
  toolApprovalMode.value = s.settings.toolApprovalMode || 'auto'
})

function applyProvider(id: string) {
  const p = providers.find(item => item.id === id)
  if (!p) return
  provider.value = p.id
  baseUrl.value = p.baseUrl
  if (!model.value || !p.models.includes(model.value)) model.value = p.models[0]
}

async function saveAll() {
  saving.value = true
  try {
    await s.save({
      provider: provider.value,
      baseUrl: baseUrl.value.trim(),
      model: model.value.trim(),
      temperature: Number(temperature.value),
      maxTokens: Number(maxTokens.value),
      toolApprovalMode: toolApprovalMode.value
    })
    if (localKey.value) {
      await s.saveKey(localKey.value)
      localKey.value = ''
    }
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
          <h2 class="text-base font-semibold text-slate-100">Agent 配置中心</h2>
          <p class="text-[11px] text-slate-500">标准大模型 Agent 客户端配置 · 支持 Provider、生成参数、工具审批和未来扩展项</p>
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
                <Cpu class="w-4 h-4 text-primary-cyan" />模型服务
              </h3>
              <p class="mt-1 text-xs text-slate-500">选择 OpenAI 兼容 Provider，并配置 API Endpoint 与默认模型。</p>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
              <button
                v-for="p in providers"
                :key="p.id"
                class="rounded-xl border p-4 text-left cursor-pointer transition"
                :class="provider === p.id ? 'border-primary/50 bg-primary/10' : 'border-white/5 glass hover:bg-white/[0.06]'"
                @click="applyProvider(p.id)"
              >
                <div class="text-sm font-medium text-slate-100">{{ p.name }}</div>
                <div class="mt-1 text-[11px] text-slate-500 truncate">{{ p.baseUrl }}</div>
              </button>
            </div>

            <div class="grid grid-cols-1 gap-4">
              <div>
                <label class="block text-[12px] text-slate-400 mb-1">Provider ID</label>
                <input v-model="provider" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
              </div>
              <div>
                <label class="block text-[12px] text-slate-400 mb-1">API Base URL</label>
                <input v-model="baseUrl" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
                <p class="mt-1 text-[11px] text-slate-500">当前预设：<code class="text-primary-cyan">{{ currentProvider.baseUrl }}</code></p>
              </div>
              <div>
                <label class="block text-[12px] text-slate-400 mb-1">默认模型</label>
                <input v-model="model" type="text" class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
                <div class="mt-2 flex flex-wrap gap-1.5">
                  <button v-for="m in currentProvider.models" :key="m" class="px-2.5 py-1 rounded-full glass text-[11px] text-slate-300 hover:bg-white/10 cursor-pointer" @click="model = m">
                    {{ m }}
                  </button>
                </div>
              </div>
            </div>
          </section>

          <section v-else-if="activeSection === 'credential'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Shield class="w-4 h-4 text-primary-cyan" />凭据与密钥
              </h3>
              <p class="mt-1 text-xs text-slate-500">API Key 仅保存在本机后端，不会回传到前端状态。</p>
            </div>

            <div class="glass rounded-xl p-4 border border-white/5">
              <div class="flex items-center gap-2 text-xs">
                <Shield class="w-3.5 h-3.5 text-primary-cyan" />
                <span :class="s.settings.hasKey ? 'text-success' : 'text-warning'">
                  {{ s.settings.hasKey ? '已保存 API Key' : '尚未配置 API Key' }}
                </span>
                <button v-if="s.settings.hasKey" class="ml-auto text-[11px] text-danger hover:underline cursor-pointer" @click="s.removeKey()">移除</button>
              </div>

              <div class="mt-3 flex items-center gap-2">
                <div class="flex-1 flex items-center gap-2 h-10 px-3 rounded-lg bg-black/30 border border-white/5">
                  <input v-model="localKey" :type="showKey ? 'text' : 'password'" placeholder="sk-... 或 DashScope API Key" class="flex-1 bg-transparent border-0 outline-none text-sm text-slate-100 placeholder:text-slate-500" />
                  <button class="p-1 rounded hover:bg-white/10 cursor-pointer" @click="showKey = !showKey">
                    <component :is="showKey ? EyeOff : Eye" class="w-4 h-4 text-slate-400" />
                  </button>
                </div>
              </div>

              <div class="mt-4 flex items-center gap-3">
                <button class="h-10 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium flex items-center gap-2 cursor-pointer hover:opacity-95 disabled:opacity-50" :disabled="s.testing" @click="s.runTest()">
                  <Loader2 v-if="s.testing" class="w-4 h-4 animate-spin" />
                  <Zap v-else class="w-4 h-4" />
                  测试连接
                </button>
                <div v-if="s.testResult" class="text-xs flex items-center gap-1.5" :class="s.testResult.ok ? 'text-success' : 'text-danger'">
                  <CheckCircle2 v-if="s.testResult.ok" class="w-3.5 h-3.5" />
                  <XCircle v-else class="w-3.5 h-3.5" />
                  {{ s.testResult.message }} <span v-if="s.testResult.ok">· {{ s.testResult.latencyMs }}ms</span>
                </div>
              </div>
            </div>
          </section>

          <section v-else-if="activeSection === 'generation'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Gauge class="w-4 h-4 text-primary-cyan" />生成参数
              </h3>
              <p class="mt-1 text-xs text-slate-500">控制模型输出的随机性、长度与后续可扩展采样参数。</p>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div class="glass rounded-xl p-4 border border-white/5">
                <label class="block text-[12px] text-slate-400 mb-2">Temperature：{{ temperature }}</label>
                <input v-model.number="temperature" type="range" min="0" max="2" step="0.1" class="w-full accent-[#7C3AED]" />
                <p class="mt-2 text-[11px] text-slate-500">越低越稳定，越高越有创造性。</p>
              </div>
              <div class="glass rounded-xl p-4 border border-white/5">
                <label class="block text-[12px] text-slate-400 mb-2">最大输出 tokens</label>
                <input v-model.number="maxTokens" type="number" min="64" max="32768" step="64" class="w-full h-10 px-3 rounded-lg bg-black/30 border border-white/5 text-sm text-slate-100 outline-none focus:border-primary/50" />
                <p class="mt-2 text-[11px] text-slate-500">限制单次模型回复的最大长度。</p>
              </div>
            </div>

            <div class="rounded-xl border border-dashed border-white/10 p-4 text-xs text-slate-500">
              后续可扩展：`top_p`、`presence_penalty`、`frequency_penalty`、`seed`、`response_format`、上下文窗口策略。
            </div>
          </section>

          <section v-else-if="activeSection === 'agent'" class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Bot class="w-4 h-4 text-primary-cyan" />Agent 能力
              </h3>
              <p class="mt-1 text-xs text-slate-500">统一管理工具调用、安全审批、Skills 和未来 MCP / Memory 能力。</p>
            </div>

            <div class="glass rounded-xl p-4 border border-white/5">
              <h4 class="text-sm font-medium text-slate-100 flex items-center gap-2"><Wrench class="w-4 h-4 text-primary-fuchsia" />工具调用审批</h4>
              <div class="mt-3 grid grid-cols-1 md:grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer" :class="toolApprovalMode === 'auto' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-slate-100">自动允许</span>
                  <span class="mt-1 block text-[11px] text-slate-500">默认模式。模型触发工具后自动执行，适合本地可信工具。</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer" :class="toolApprovalMode === 'manual' ? 'border-primary/50 bg-primary/10' : 'border-white/5 bg-black/20'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-slate-100">敏感工具确认</span>
                  <span class="mt-1 block text-[11px] text-slate-500">仅 `requiresApproval` 工具会等待用户确认。</span>
                </label>
              </div>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div class="rounded-xl border border-dashed border-white/10 p-4 text-xs text-slate-500">
                <div class="text-sm text-slate-300 mb-1">Skills</div>
                当前通过 Skills 技能库管理，已支持外部 zip 导入。后续可扩展启用策略、优先级和作用域。
              </div>
              <div class="rounded-xl border border-dashed border-white/10 p-4 text-xs text-slate-500">
                <div class="text-sm text-slate-300 mb-1">MCP / 外部工具</div>
                预留 MCP Server、远程工具、沙箱权限、工具分组等配置入口。
              </div>
            </div>
          </section>

          <section v-else class="space-y-5">
            <div>
              <h3 class="text-sm font-semibold text-slate-100 flex items-center gap-2">
                <Database class="w-4 h-4 text-primary-cyan" />运行时与存储
              </h3>
              <p class="mt-1 text-xs text-slate-500">展示当前存储与网络运行方式，预留未来同步、代理和日志配置。</p>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div class="glass rounded-xl p-4 border border-white/5">
                <div class="flex items-center gap-2 text-sm text-slate-100"><Database class="w-4 h-4 text-primary-fuchsia" />本地数据</div>
                <p class="mt-2 text-xs text-slate-500">配置、API Key 和会话记录保存在系统用户数据目录 `PointerApp` 下。</p>
              </div>
              <div class="glass rounded-xl p-4 border border-white/5">
                <div class="flex items-center gap-2 text-sm text-slate-100"><Network class="w-4 h-4 text-primary-fuchsia" />网络</div>
                <p class="mt-2 text-xs text-slate-500">当前直接访问模型 API。后续可扩展代理、超时、重试、限流配置。</p>
              </div>
            </div>
          </section>
        </main>
      </div>

      <footer class="px-5 h-14 flex items-center gap-3 border-t border-white/5 shrink-0">
        <p class="text-[11px] text-slate-500 flex-1">
          当前仅保存已实现字段；虚线区域为后续 Agent 配置扩展位。
        </p>
        <button class="h-9 px-4 rounded-lg glass hover:bg-white/10 text-sm text-slate-200 cursor-pointer" @click="emit('close')">取消</button>
        <button class="h-9 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50" :disabled="saving" @click="saveAll">
          {{ saving ? '保存中…' : '保存配置' }}
        </button>
      </footer>
    </div>
  </div>
</template>
