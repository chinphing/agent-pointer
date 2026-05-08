import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getSettings, updateSettings, setApiKey, clearApiKey, testConnection } from '../lib/api'
import type { ModelSettings } from '../types/chat'

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<ModelSettings>({
    provider: 'qwen',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    model: 'qwen-plus',
    apiKey: '',
    temperature: 0.7,
    maxTokens: 2048,
    hasKey: false,
    toolApprovalMode: 'auto',
    agentMode: 'single'
  })
  const loading = ref(false)
  const testing = ref(false)
  const testResult = ref<{ ok: boolean; latencyMs: number; message: string } | null>(null)

  async function load() {
    loading.value = true
    const s = await getSettings().catch(() => null)
    if (s) settings.value = s
    loading.value = false
  }

  async function save(patch: Partial<ModelSettings>) {
    const merged: ModelSettings = { ...settings.value, ...patch }
    const updated = await updateSettings(merged)
    settings.value = updated
  }

  async function saveKey(key: string) {
    await setApiKey(key)
    settings.value.hasKey = !!key
  }

  async function removeKey() {
    await clearApiKey()
    settings.value.hasKey = false
  }

  async function runTest() {
    testing.value = true
    testResult.value = null
    const r = await testConnection().catch(e => ({ ok: false, latencyMs: 0, message: String(e) }))
    testResult.value = r
    testing.value = false
  }

  return { settings, loading, testing, testResult, load, save, saveKey, removeKey, runTest }
})
