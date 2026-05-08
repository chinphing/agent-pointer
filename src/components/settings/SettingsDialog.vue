<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { X, Eye, EyeOff, Zap, Shield, CheckCircle2, XCircle, Loader2 } from 'lucide-vue-next'
import { useSettingsStore } from '../../stores/settings'

defineEmits<{ (e: 'close'): void }>()
const s = useSettingsStore()

const localKey = ref('')
const showKey = ref(false)
const saving = ref(false)

const baseUrl = ref('')
const model = ref('')
const temperature = ref(0.7)
const maxTokens = ref(2048)

onMounted(() => {
  baseUrl.value = s.settings.baseUrl
  model.value = s.settings.model
  temperature.value = s.settings.temperature
  maxTokens.value = s.settings.maxTokens
})

async function saveAll() {
  saving.value = true
  await s.save({
    baseUrl: baseUrl.value.trim(),
    model: model.value.trim(),
    temperature: Number(temperature.value),
    maxTokens: Number(maxTokens.value)
  })
  if (localKey.value) {
    await s.saveKey(localKey.value)
    localKey.value = ''
  }
  saving.value = false
}

const presets = [
  { label: 'qwen-plus', model: 'qwen-plus' },
  { label: 'qwen-turbo', model: 'qwen-turbo' },
  { label: 'qwen-max', model: 'qwen-max' },
  { label: 'qwen2.5-coder-32b', model: 'qwen2.5-coder-32b-instruct' }
]
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="$emit('close')">
    <div class="w-[680px] max-w-[92vw] max-h-[88vh] glass-strong rounded-2xl border border-white/10 shadow-2xl flex flex-col overflow-hidden">
      <header class="px-5 h-14 flex items-center gap-2 border-b border-white/5">
        <h2 class="text-base font-semibold text-slate-100">设置</h2>
        <span class="text-xs text-slate-500">默认接入 阿里云千问 · OpenAI 兼容模式</span>
        <div class="flex-1" />
        <button class="p-2 rounded-lg hover:bg-white/5 cursor-pointer" @click="$emit('close')">
          <X class="w-4 h-4 text-slate-300" />
        </button>
      </header>

      <div class="flex-1 overflow-y-auto p-5 space-y-6">
        <!-- Provider -->
        <section>
          <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-400 mb-2">模型配置</h3>
          <div class="space-y-3">
            <div>
              <label class="block text-[12px] text-slate-400 mb-1">API Base URL</label>
              <input v-model="baseUrl" type="text"
                     class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
              <p class="mt-1 text-[11px] text-slate-500">
                千问默认：<code class="text-primary-cyan">https://dashscope.aliyuncs.com/compatible-mode/v1</code>
              </p>
            </div>

            <div>
              <label class="block text-[12px] text-slate-400 mb-1">模型</label>
              <input v-model="model" type="text"
                     class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
              <div class="mt-2 flex flex-wrap gap-1.5">
                <button v-for="p in presets" :key="p.model"
                        class="px-2.5 py-1 rounded-full glass text-[11px] text-slate-300 hover:bg-white/10 cursor-pointer"
                        @click="model = p.model">{{ p.label }}</button>
              </div>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div>
                <label class="block text-[12px] text-slate-400 mb-1">温度 ({{ temperature }})</label>
                <input v-model.number="temperature" type="range" min="0" max="2" step="0.1" class="w-full accent-[#7C3AED]" />
              </div>
              <div>
                <label class="block text-[12px] text-slate-400 mb-1">最大输出 tokens</label>
                <input v-model.number="maxTokens" type="number" min="64" max="32768" step="64"
                       class="w-full h-10 px-3 rounded-lg glass border border-white/5 bg-transparent text-sm text-slate-100 outline-none focus:border-primary/50" />
              </div>
            </div>
          </div>
        </section>

        <!-- API Key -->
        <section>
          <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-400 mb-2">API Key</h3>
          <div class="glass rounded-xl p-3 border border-white/5">
            <div class="flex items-center gap-2 text-xs">
              <Shield class="w-3.5 h-3.5 text-primary-cyan" />
              <span :class="s.settings.hasKey ? 'text-success' : 'text-warning'">
                {{ s.settings.hasKey ? '已保存到本地（密钥不会回传到前端）' : '尚未配置 API Key' }}
              </span>
              <button v-if="s.settings.hasKey" class="ml-auto text-[11px] text-danger hover:underline cursor-pointer"
                      @click="s.removeKey()">移除</button>
            </div>
            <div class="mt-2 flex items-center gap-2">
              <div class="flex-1 flex items-center gap-2 h-10 px-3 rounded-lg bg-black/30 border border-white/5">
                <input
                  v-model="localKey"
                  :type="showKey ? 'text' : 'password'"
                  placeholder="sk-... 或 DashScope API Key"
                  class="flex-1 bg-transparent border-0 outline-none text-sm text-slate-100 placeholder:text-slate-500"
                />
                <button class="p-1 rounded hover:bg-white/10 cursor-pointer" @click="showKey = !showKey">
                  <component :is="showKey ? EyeOff : Eye" class="w-4 h-4 text-slate-400" />
                </button>
              </div>
            </div>
            <p class="mt-2 text-[11px] text-slate-500">在阿里云百炼控制台 → API-KEY 管理 中生成。</p>
          </div>
        </section>

        <!-- Test -->
        <section>
          <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-400 mb-2">连接测试</h3>
          <div class="flex items-center gap-3">
            <button
              class="h-10 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium flex items-center gap-2 cursor-pointer hover:opacity-95 disabled:opacity-50 disabled:cursor-not-allowed"
              :disabled="s.testing"
              @click="s.runTest()"
            >
              <Loader2 v-if="s.testing" class="w-4 h-4 animate-spin" />
              <Zap v-else class="w-4 h-4" />
              测试连接
            </button>
            <div v-if="s.testResult" class="text-xs flex items-center gap-1.5"
                 :class="s.testResult.ok ? 'text-success' : 'text-danger'">
              <CheckCircle2 v-if="s.testResult.ok" class="w-3.5 h-3.5" />
              <XCircle v-else class="w-3.5 h-3.5" />
              {{ s.testResult.message }} <span v-if="s.testResult.ok">· {{ s.testResult.latencyMs }}ms</span>
            </div>
          </div>
        </section>
      </div>

      <footer class="px-5 h-14 flex items-center gap-3 border-t border-white/5">
        <p class="text-[11px] text-slate-500 flex-1">
          API Key 仅保存在本机；工具调用默认自动允许，也可在设置中改为敏感工具二次确认。
        </p>
        <button class="h-9 px-4 rounded-lg glass hover:bg-white/10 text-sm text-slate-200 cursor-pointer" @click="$emit('close')">取消</button>
        <button
          class="h-9 px-4 rounded-lg bg-gradient-to-r from-primary to-primary-fuchsia text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50"
          :disabled="saving"
          @click="saveAll"
        >{{ saving ? '保存中…' : '保存' }}</button>
      </footer>
    </div>
  </div>
</template>
